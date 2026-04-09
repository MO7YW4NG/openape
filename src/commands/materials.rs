use anyhow::Result;
use crate::Cli;
use crate::moodle::course::{get_enrolled_courses_api, get_site_info};
use crate::moodle::material::{get_resources_by_courses_api, get_incomplete_completions, view_resource_api};
use crate::moodle::video::update_completion_status;
use crate::output::format_and_output;
use crate::utils::{sanitize_filename, format_file_size};
use std::path::Path;
use super::{ApiCtx, in_progress_all_to_classification, level_to_classification};

pub async fn run(cmd: &crate::MaterialsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::MaterialsCommands::ListAll { level } => {
            let classification = in_progress_all_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            let course_map: std::collections::HashMap<u64, &str> = courses.iter()
                .map(|c| (c.id, c.fullname.as_str()))
                .collect();

            let items: Vec<serde_json::Value> = resources.iter().map(|r| serde_json::json!({
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
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &[*course_id]).await?;
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
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            if resources.is_empty() {
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

            let summary = download_resources(&ctx, &resources, output_dir, ws_token).await?;

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
            let site_info = get_site_info(&ctx.client, &ctx.session).await?;
            let incompletes = get_incomplete_completions(&ctx.client, &ctx.session, *course_id, site_info.userid).await?;
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
            let site_info = get_site_info(&ctx.client, &ctx.session).await?;

            let mut all_incomplete = Vec::new();
            for course in &courses {
                if let Ok(items) = get_incomplete_completions(&ctx.client, &ctx.session, course.id, site_info.userid).await {
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
        if resource.mod_type != "resource" {
            summary.skipped += 1;
            summary.files.push(serde_json::json!({
                "name": resource.name,
                "status": "skipped",
                "reason": "not_resource",
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

        let mut filename = sanitize_filename(&resource.name, 100);
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

        // Append token to URL for authenticated download
        let url = if resource.url.contains('?') {
            format!("{}&token={}", resource.url, ws_token)
        } else {
            format!("{}?token={}", resource.url, ws_token)
        };

        match ctx.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await?;
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
