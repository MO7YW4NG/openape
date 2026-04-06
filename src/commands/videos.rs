use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::video::{get_supervideos_in_course_api, get_incomplete_videos_api, update_completion_status};
use crate::output::format_and_output;
use super::ApiCtx;

pub async fn run(cmd: &crate::VideosCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::VideosCommands::List { course_id, incomplete_only } => {
            let mut videos = get_supervideos_in_course_api(&ctx.client, &ctx.session, *course_id).await?;
            if *incomplete_only {
                videos.retain(|v| !v.is_complete);
            }

            let items: Vec<serde_json::Value> = videos.iter().map(|v| serde_json::json!({
                "cmid": v.cmid,
                "name": v.name,
                "url": v.url,
                "is_complete": v.is_complete,
            })).collect();

            ctx.log.info(&format!("Found {} videos", items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::VideosCommands::Complete { course_id, dry_run } => {
            let videos = get_incomplete_videos_api(&ctx.client, &ctx.session, *course_id).await?;

            if videos.is_empty() {
                ctx.log.info("All videos already complete (or no videos found).");
                return Ok(());
            }

            ctx.log.info(&format!("Found {} incomplete videos", videos.len()));

            if *dry_run {
                for v in &videos {
                    ctx.log.info(&format!("  [dry-run] {}", v.name));
                }
                ctx.log.info(&format!("Would complete {} videos", videos.len()));
                return Ok(());
            }

            let mut completed = 0;
            for v in &videos {
                let cmid: u64 = v.cmid.parse().unwrap_or(0);
                if cmid == 0 { continue; }
                match update_completion_status(&ctx.client, &ctx.session, cmid, true).await {
                    Ok(true) => {
                        ctx.log.success(&format!("  Completed: {}", v.name));
                        completed += 1;
                    }
                    Ok(false) => ctx.log.warn(&format!("  Failed (no status): {}", v.name)),
                    Err(e) => ctx.log.warn(&format!("  Error completing {}: {}", v.name, e)),
                }
            }
            ctx.log.info(&format!("Completed {}/{} videos", completed, videos.len()));
        }

        crate::VideosCommands::CompleteAll { dry_run } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;
            ctx.log.info(&format!("Scanning {} courses for incomplete videos...", courses.len()));

            let mut total_completed = 0;
            let mut total_found = 0;

            for course in &courses {
                let videos = match get_incomplete_videos_api(&ctx.client, &ctx.session, course.id).await {
                    Ok(v) => v,
                    Err(e) => {
                        ctx.log.warn(&format!("  Skipping {}: {}", course.fullname, e));
                        continue;
                    }
                };
                if videos.is_empty() { continue; }
                total_found += videos.len();
                ctx.log.info(&format!("  {}: {} incomplete", course.fullname, videos.len()));

                if !dry_run {
                    for v in &videos {
                        let cmid: u64 = v.cmid.parse().unwrap_or(0);
                        if cmid == 0 { continue; }
                        if let Ok(true) = update_completion_status(&ctx.client, &ctx.session, cmid, true).await {
                            ctx.log.success(&format!("    Completed: {}", v.name));
                            total_completed += 1;
                        }
                    }
                }
            }

            if *dry_run {
                ctx.log.info(&format!("Would complete {} videos", total_found));
            } else {
                ctx.log.info(&format!("Completed {}/{} videos", total_completed, total_found));
            }
        }

        crate::VideosCommands::Download { course_id, output_dir: _, incomplete_only } => {
            let mut videos = get_supervideos_in_course_api(&ctx.client, &ctx.session, *course_id).await?;
            if *incomplete_only {
                videos.retain(|v| !v.is_complete);
            }
            if videos.is_empty() {
                ctx.log.info("No videos to download.");
                return Ok(());
            }
            ctx.log.warn("Video download requires a browser session (not yet implemented).");
            for v in &videos {
                ctx.log.info(&format!("  - {} (cmid: {})", v.name, v.cmid));
            }
        }
    }

    Ok(())
}
