use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::material::get_resources_by_courses_api;
use crate::moodle::video::update_completion_status;
use crate::output::format_and_output;
use crate::utils::{sanitize_filename, format_file_size};
use std::path::Path;
use super::{ApiCtx, level_to_classification};

pub async fn run(cmd: &crate::MaterialsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::MaterialsCommands::ListAll { level } => {
            let classification = level_to_classification(*level);
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
                return Ok(());
            }

            let ws_token = ctx.session.ws_token.as_ref()
                .ok_or_else(|| anyhow::anyhow!("WS token required"))?;

            download_resources(&ctx, &resources, output_dir, ws_token).await?;
        }

        crate::MaterialsCommands::DownloadAll { output_dir, level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            if resources.is_empty() {
                ctx.log.info("No materials found.");
                return Ok(());
            }

            let ws_token = ctx.session.ws_token.as_ref()
                .ok_or_else(|| anyhow::anyhow!("WS token required"))?;

            download_resources(&ctx, &resources, output_dir, ws_token).await?;
        }

        crate::MaterialsCommands::Complete { course_id, dry_run } => {
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &[*course_id]).await?;
            ctx.log.info(&format!("Found {} resources", resources.len()));

            if *dry_run {
                for r in &resources {
                    ctx.log.info(&format!("  [dry-run] {}", r.name));
                }
                return Ok(());
            }

            let mut completed = 0;
            for r in &resources {
                let cmid: u64 = r.cmid.parse().unwrap_or(0);
                if cmid == 0 { continue; }
                if let Ok(true) = update_completion_status(&ctx.client, &ctx.session, cmid, true).await {
                    ctx.log.success(&format!("  Completed: {}", r.name));
                    completed += 1;
                }
            }
            ctx.log.info(&format!("Completed {}/{} resources", completed, resources.len()));
        }

        crate::MaterialsCommands::CompleteAll { dry_run, level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let resources = get_resources_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            ctx.log.info(&format!("Found {} resources across {} courses", resources.len(), courses.len()));

            if *dry_run {
                for r in &resources {
                    ctx.log.info(&format!("  [dry-run] {}", r.name));
                }
                ctx.log.info(&format!("Would complete {} resources", resources.len()));
                return Ok(());
            }

            let mut completed = 0;
            for r in &resources {
                let cmid: u64 = r.cmid.parse().unwrap_or(0);
                if cmid == 0 { continue; }
                if let Ok(true) = update_completion_status(&ctx.client, &ctx.session, cmid, true).await {
                    completed += 1;
                }
            }
            ctx.log.info(&format!("Completed {}/{} resources", completed, resources.len()));
        }
    }

    Ok(())
}

async fn download_resources(
    ctx: &ApiCtx,
    resources: &[crate::moodle::types::ResourceModule],
    output_dir: &std::path::PathBuf,
    ws_token: &str,
) -> Result<()> {
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

    let mut downloaded = 0;
    let mut skipped = 0;

    for resource in resources {
        if resource.mod_type != "resource" {
            skipped += 1;
            continue;
        }
        if resource.url.is_empty() {
            ctx.log.warn(&format!("  No URL for: {}", resource.name));
            continue;
        }

        std::fs::create_dir_all(output_dir)?;

        let mut filename = sanitize_filename(&resource.name, 100);
        if Path::new(&filename).extension().is_none() {
            if let Some(ext) = resource.mimetype.as_deref().and_then(|m| mime_to_ext.get(m)) {
                filename.push_str(ext);
            }
        }

        let dest = output_dir.join(&filename);
        if dest.exists() {
            ctx.log.info(&format!("  Skip (exists): {}", filename));
            skipped += 1;
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
                ctx.log.success(&format!("  Downloaded: {} ({})", filename, format_file_size(bytes.len() as u64, 1)));
                downloaded += 1;
            }
            Ok(resp) => {
                ctx.log.warn(&format!("  HTTP {} for: {}", resp.status(), resource.name));
            }
            Err(e) => {
                ctx.log.warn(&format!("  Download failed for {}: {}", resource.name, e));
            }
        }
    }

    ctx.log.info(&format!("Downloaded: {}, Skipped: {}", downloaded, skipped));
    Ok(())
}
