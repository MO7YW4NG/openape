use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::material::{get_course_contents_resources, get_incomplete_completions, view_resource_api, resolve_pdfannotator_urls};
use crate::moodle::video::update_completion_status;
use crate::output::format_and_output;
use crate::utils::{sanitize_filename, format_file_size};
use std::path::Path;
use super::{ApiCtx, in_progress_all_to_classification, level_to_classification};

pub async fn run(cmd: &crate::MaterialsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::MaterialsCommands::List { course_id } => {
            let resources = get_course_contents_resources(&ctx.client, &ctx.session, *course_id).await?;

            let items: Vec<serde_json::Value> = resources.iter().map(|r| serde_json::json!({
                "course_id": r.course_id,
                "cmid": r.cmid,
                "name": r.name,
                "url": r.url,
                "mod_type": r.mod_type,
                "mimetype": r.mimetype,
                "filesize": r.filesize.map(|s| format_file_size(s, 1)),
            })).collect();

            ctx.log.info(&format!("Found {} materials in course {}", items.len(), course_id));
            format_and_output(&items, ctx.output, None);
        }

        crate::MaterialsCommands::ListAll { level } => {
            let classification = in_progress_all_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;

            let all_resources = fetch_all_resources(&ctx, &courses).await;

            let course_map: std::collections::HashMap<u64, &str> = courses.iter()
                .map(|c| (c.id, c.fullname.as_str()))
                .collect();

            let items: Vec<serde_json::Value> = all_resources.iter().map(|r| serde_json::json!({
                "course_id": r.course_id,
                "course_name": course_map.get(&r.course_id).copied().unwrap_or("Unknown"),
                "cmid": r.cmid,
                "name": r.name,
                "url": r.url,
                "mod_type": r.mod_type,
                "mimetype": r.mimetype,
                "filesize": r.filesize.map(|s| format_file_size(s, 1)),
            })).collect();

            ctx.log.info(&format!("Found {} materials across {} courses", items.len(), courses.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::MaterialsCommands::Download { course_id, output_dir } => {
            let mut resources = get_course_contents_resources(&ctx.client, &ctx.session, *course_id).await?;
            if resources.is_empty() {
                ctx.log.info("No materials found.");
                let result = serde_json::json!({
                    "action": "download",
                    "course_id": *course_id,
                    "output_dir": output_dir.to_string_lossy(),
                    "downloaded": 0,
                    "skipped": 0,
                    "failed": 0,
                    "files": [],
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            let ws_token = ctx.session.ws_token.as_ref()
                .ok_or_else(|| anyhow::anyhow!("WS token required"))?;

            // Resolve pdfannotator URLs via headless browser
            resources = resolve_pdfannotator_in_resources(resources, cli).await?;

            let summary = download_resources(&ctx, &resources, output_dir, ws_token).await?;

            let result = serde_json::json!({
                "action": "download",
                "course_id": *course_id,
                "output_dir": output_dir.to_string_lossy(),
                "downloaded": summary.downloaded,
                "skipped": summary.skipped,
                "failed": summary.failed,
                "files": summary.files,
            });
            format_and_output(&[result], ctx.output, None);
        }

        crate::MaterialsCommands::DownloadAll { output_dir, level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;

            let mut all_resources = fetch_all_resources(&ctx, &courses).await;

            if all_resources.is_empty() {
                ctx.log.info("No materials found.");
                let result = serde_json::json!({
                    "action": "download_all",
                    "courses_scanned": courses.len(),
                    "output_dir": output_dir.to_string_lossy(),
                    "downloaded": 0,
                    "skipped": 0,
                    "failed": 0,
                    "files": [],
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            let ws_token = ctx.session.ws_token.as_ref()
                .ok_or_else(|| anyhow::anyhow!("WS token required"))?;

            // Resolve pdfannotator URLs via headless browser
            all_resources = resolve_pdfannotator_in_resources(all_resources, cli).await?;

            let summary = download_resources(&ctx, &all_resources, output_dir, ws_token).await?;

            let result = serde_json::json!({
                "action": "download_all",
                "courses_scanned": courses.len(),
                "output_dir": output_dir.to_string_lossy(),
                "downloaded": summary.downloaded,
                "skipped": summary.skipped,
                "failed": summary.failed,
                "files": summary.files,
            });
            format_and_output(&[result], ctx.output, None);
        }

        crate::MaterialsCommands::Complete { course_id, dry_run } => {
            let incompletes = get_incomplete_completions(&ctx.client, &ctx.session, *course_id, ctx.session.user_id).await?;
            // Filter out supervideo modules (handled by videos command)
            let incompletes: Vec<_> = incompletes.into_iter().filter(|i| i.modname != "supervideo").collect();
            let total = incompletes.len();
            ctx.log.info(&format!("Found {} incomplete resources", total));

            if total == 0 {
                let result = serde_json::json!({
                    "action": "complete", "course_id": *course_id,
                    "total": 0, "completed": 0, "failed": 0, "dry_run": *dry_run,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            if *dry_run {
                for i in &incompletes {
                    ctx.log.info(&format!("  [dry-run] {} ({}: rule={:?})", i.name, i.modname, i.rule));
                }
                let result = serde_json::json!({
                    "action": "complete", "course_id": *course_id,
                    "total": total, "completed": 0, "failed": 0, "dry_run": true,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            let mut completed = 0;
            for i in &incompletes {
                let success = match i.rule.as_deref() {
                    Some("completionview") if i.modname == "resource" => {
                        // Use mod_resource_view_resource for view-based completion
                        view_resource_api(&ctx.client, &ctx.session, i.instance).await
                            .unwrap_or(false)
                    }
                    _ => {
                        // Try manual completion API
                        match update_completion_status(&ctx.client, &ctx.session, i.cmid, true).await {
                            Ok(true) => true,
                            Ok(false) | Err(_) => false,
                        }
                    }
                };

                if success {
                    ctx.log.success(&format!("  Completed: {}", i.name));
                    completed += 1;
                } else {
                    ctx.log.warn(&format!("  Failed: {} ({}: rule={:?})", i.name, i.modname, i.rule));
                }
            }
            ctx.log.info(&format!("Completed {}/{} resources", completed, total));
            let result = serde_json::json!({
                "action": "complete",
                "course_id": *course_id,
                "total": total,
                "completed": completed,
                "failed": total - completed,
                "dry_run": false,
            });
            format_and_output(&[result], ctx.output, None);
        }

        crate::MaterialsCommands::CompleteAll { dry_run, level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;

            let mut all_incomplete = Vec::new();
            for course in &courses {
                if let Ok(items) = get_incomplete_completions(&ctx.client, &ctx.session, course.id, ctx.session.user_id).await {
                    all_incomplete.extend(items.into_iter().filter(|i| i.modname != "supervideo"));
                }
            }

            let total = all_incomplete.len();
            ctx.log.info(&format!("Found {} incomplete resources across {} courses", total, courses.len()));

            if *dry_run {
                for i in &all_incomplete {
                    ctx.log.info(&format!("  [dry-run] {} ({}: rule={:?})", i.name, i.modname, i.rule));
                }
                let result = serde_json::json!({
                    "action": "complete_all",
                    "courses_scanned": courses.len(),
                    "total": total, "completed": 0, "dry_run": true,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            let mut completed = 0;
            for i in &all_incomplete {
                let success = match i.rule.as_deref() {
                    Some("completionview") if i.modname == "resource" => {
                        view_resource_api(&ctx.client, &ctx.session, i.instance).await
                            .unwrap_or(false)
                    }
                    _ => {
                        match update_completion_status(&ctx.client, &ctx.session, i.cmid, true).await {
                            Ok(true) => true,
                            Ok(false) | Err(_) => false,
                        }
                    }
                };

                if success {
                    ctx.log.success(&format!("  Completed: {}", i.name));
                    completed += 1;
                } else {
                    ctx.log.warn(&format!("  Failed: {} ({}: rule={:?})", i.name, i.modname, i.rule));
                }
            }
            ctx.log.info(&format!("Completed {}/{} resources", completed, total));
            let result = serde_json::json!({
                "action": "complete_all",
                "courses_scanned": courses.len(),
                "total": total,
                "completed": completed,
                "failed": total - completed,
                "dry_run": false,
            });
            format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}

async fn fetch_all_resources(
    ctx: &ApiCtx,
    courses: &[crate::moodle::types::EnrolledCourse],
) -> Vec<crate::moodle::types::ResourceModule> {
    let mut all = Vec::new();
    for course in courses {
        match get_course_contents_resources(&ctx.client, &ctx.session, course.id).await {
            Ok(resources) => all.extend(resources),
            Err(e) => ctx.log.warn(&format!("Failed to fetch materials for {}: {}", course.fullname, e)),
        }
    }
    all
}

async fn resolve_pdfannotator_in_resources(
    mut resources: Vec<crate::moodle::types::ResourceModule>,
    cli: &Cli,
) -> Result<Vec<crate::moodle::types::ResourceModule>> {
    use crate::config::load_config;
    use crate::auth::load_cookies;

    // Use view_url for headless browser, fall back to url if view_url unavailable
    let pdfannotators: Vec<(String, String, String)> = resources.iter()
        .filter(|r| r.mod_type == "pdfannotator")
        .map(|r| {
            let visit_url = r.view_url.as_deref().unwrap_or(&r.url);
            (r.cmid.clone(), r.name.clone(), visit_url.to_string())
        })
        .filter(|(_, _, url)| !url.is_empty())
        .collect();

    if pdfannotators.is_empty() {
        return Ok(resources);
    }

    let config = load_config(Some(&cli.session));
    let log = crate::logger::Logger::new(cli.verbose, cli.silent);
    log.info(&format!("Resolving {} pdfannotator URL(s) via headless browser...", pdfannotators.len()));

    let cookies = match load_cookies(&config.auth_state_path) {
        Ok(c) if !c.is_empty() => c,
        _ => {
            log.warn("No saved cookies found, skipping headless resolution");
            return Ok(resources);
        }
    };

    match resolve_pdfannotator_urls(&pdfannotators, &cookies, &config.moodle_base_url).await {
        Ok(resolved) => {
            for resource in &mut resources {
                if resource.mod_type == "pdfannotator" {
                    if let Some(url) = resolved.get(&resource.cmid) {
                        resource.url = url.clone();
                    }
                }
            }
            log.info(&format!("Resolved {}/{} pdfannotator URL(s)", resolved.len(), pdfannotators.len()));
        }
        Err(e) => {
            log.warn(&format!("Headless resolution failed: {}", e));
        }
    }

    Ok(resources)
}

struct DownloadSummary {
    downloaded: usize,
    skipped: usize,
    failed: usize,
    files: Vec<serde_json::Value>,
}

async fn download_resources(
    ctx: &ApiCtx,
    resources: &[crate::moodle::types::ResourceModule],
    output_dir: &std::path::PathBuf,
    ws_token: &str,
) -> Result<DownloadSummary> {
    let mime_to_ext: std::collections::HashMap<&str, &str> = [
        ("application/pdf", ".pdf"),
        ("application/vnd.ms-powerpoint", ".ppt"),
        ("application/vnd.openxmlformats-officedocument.presentationml.presentation", ".pptx"),
        ("application/msword", ".doc"),
        ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", ".docx"),
        ("application/vnd.ms-excel", ".xls"),
        ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", ".xlsx"),
        ("application/zip", ".zip"),
        ("image/jpeg", ".jpg"),
        ("image/png", ".png"),
    ].into_iter().collect();

    let mut summary = DownloadSummary {
        downloaded: 0,
        skipped: 0,
        failed: 0,
        files: Vec::new(),
    };

    std::fs::create_dir_all(output_dir)?;

    for resource in resources {
        if resource.mod_type != "resource" && resource.mod_type != "pdfannotator" {
            summary.skipped += 1;
            summary.files.push(serde_json::json!({
                "name": resource.name,
                "status": "skipped",
                "reason": "unsupported_type",
            }));
            continue;
        }
        if resource.url.is_empty() {
            ctx.log.warn(&format!("  No URL for: {}", resource.name));
            summary.files.push(serde_json::json!({
                "name": resource.name,
                "status": "failed",
                "error": "no_url",
            }));
            summary.failed += 1;
            continue;
        }

        let mut filename = if resource.mod_type == "pdfannotator" {
            resource.url.rsplit('/').next()
                .map(crate::utils::percent_decode)
                .map(|s| sanitize_filename(&s, 100))
                .unwrap_or_else(|| sanitize_filename(&resource.name, 100))
        } else {
            sanitize_filename(&resource.name, 100)
        };
        if Path::new(&filename).extension().is_none() {
            if let Some(ext) = resource.mimetype.as_deref().and_then(|m| mime_to_ext.get(m)) {
                filename.push_str(ext);
            }
        }

        let dest = output_dir.join(&filename);
        if dest.exists() {
            ctx.log.info(&format!("  Skip (exists): {}", filename));
            summary.skipped += 1;
            summary.files.push(serde_json::json!({
                "name": filename,
                "status": "skipped",
                "reason": "exists",
            }));
            continue;
        }

        let url = if resource.url.contains('?') {
            format!("{}&token={}", resource.url, ws_token)
        } else {
            format!("{}?token={}", resource.url, ws_token)
        };

        ctx.log.debug(&format!("  Downloading: {}", url));
        match ctx.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await?;
                // Validate: PDF files should start with %PDF, reject JSON error responses
                if filename.ends_with(".pdf") && !bytes.starts_with(b"%PDF") {
                    let preview = String::from_utf8_lossy(&bytes);
                    ctx.log.warn(&format!("  Invalid PDF: {} ({})", resource.name, &preview[..preview.len().min(80)]));
                    summary.files.push(serde_json::json!({
                        "name": resource.name,
                        "status": "failed",
                        "error": "invalid_content",
                    }));
                    summary.failed += 1;
                } else {
                    std::fs::write(&dest, &bytes)?;
                    let size = format_file_size(bytes.len() as u64, 1);
                    ctx.log.success(&format!("  Downloaded: {} ({})", filename, size));
                    summary.downloaded += 1;
                    summary.files.push(serde_json::json!({
                        "name": filename,
                        "path": dest.to_string_lossy(),
                        "size": size,
                        "status": "downloaded",
                    }));
                }
            }
            Ok(resp) => {
                let status = resp.status().to_string();
                ctx.log.warn(&format!("  HTTP {} for: {}", status, resource.name));
                summary.files.push(serde_json::json!({
                    "name": resource.name,
                    "status": "failed",
                    "error": format!("HTTP {}", status),
                }));
                summary.failed += 1;
            }
            Err(e) => {
                ctx.log.warn(&format!("  Download failed for {}: {}", resource.name, e));
                summary.files.push(serde_json::json!({
                    "name": resource.name,
                    "status": "failed",
                    "error": e.to_string(),
                }));
                summary.failed += 1;
            }
        }
    }

    ctx.log.info(&format!("Downloaded: {}, Skipped: {}", summary.downloaded, summary.skipped));
    Ok(summary)
}
