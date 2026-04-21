use anyhow::Result;
use crate::Cli;
use crate::config::load_config;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::video::{get_supervideos_in_course_api, get_incomplete_videos_api, update_completion_status, get_video_metadata_browser, download_video_with_cookies, save_video_progress_api, VideoMetadata};
use crate::output::format_and_output;
use crate::utils::sanitize_filename;
use super::ApiCtx;
use crate::moodle::types::SuperVideoModule;

pub async fn run(cmd: &crate::VideosCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

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
            let total = videos.len();

            if videos.is_empty() {
                ctx.log.info("All videos already complete (or no videos found).");
                let result = serde_json::json!({
                    "action": "complete",
                    "course_id": *course_id,
                    "total": 0,
                    "completed": 0,
                    "failed": 0,
                    "dry_run": *dry_run,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            ctx.log.info(&format!("Found {} incomplete videos", total));

            if *dry_run {
                for v in &videos {
                    ctx.log.info(&format!("  [dry-run] {}", v.name));
                }
                ctx.log.info(&format!("Would complete {} videos", total));
                let result = serde_json::json!({
                    "action": "complete",
                    "course_id": *course_id,
                    "total": total,
                    "completed": 0,
                    "failed": 0,
                    "dry_run": true,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            // Launch browser only for getting metadata
            let config = load_config(cli.config.as_ref().and_then(|p| p.parent()));
            let launched = crate::auth::launch_persistent_session(&config, &ctx.log, true).await?;

            let mut completed = 0;
            for v in &videos {
                ctx.log.info(&format!("Processing: {}", v.name));

                // 1. Get metadata via browser
                let metadata = match get_video_metadata_browser(&launched.page, &v.url, &ctx.log).await {
                    Ok(m) => m,
                    Err(e) => {
                        ctx.log.warn(&format!("  Failed to get metadata: {}", e));
                        continue;
                    }
                };

                // 2. Complete via API
                match complete_video(&ctx, &v, &metadata).await {
                    Ok(()) => {
                        ctx.log.success(&format!("  Completed: {}", v.name));
                        completed += 1;
                    }
                    Err(e) => {
                        ctx.log.warn(&format!("  Failed to complete: {} — {}", v.name, e));
                    }
                }
            }

            crate::auth::close_persistent_session(launched).await;
            ctx.log.info(&format!("Completed {}/{} videos", completed, total));
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

        crate::VideosCommands::CompleteAll { dry_run } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;
            ctx.log.info(&format!("Scanning {} courses for incomplete videos...", courses.len()));

            let mut all_incomplete = Vec::new();
            for course in &courses {
                if let Ok(videos) = get_incomplete_videos_api(&ctx.client, &ctx.session, course.id).await {
                    for v in videos {
                        all_incomplete.push((course.fullname.clone(), v));
                    }
                }
            }

            let total = all_incomplete.len();

            if all_incomplete.is_empty() {
                ctx.log.info("No incomplete videos found.");
                let result = serde_json::json!({
                    "action": "complete_all",
                    "courses_scanned": courses.len(),
                    "total": 0,
                    "completed": 0,
                    "failed": 0,
                    "dry_run": *dry_run,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            ctx.log.info(&format!("Found {} incomplete videos across all courses", total));

            if *dry_run {
                for (cname, v) in &all_incomplete {
                    ctx.log.info(&format!("  [dry-run] [{}]: {}", cname, v.name));
                }
                ctx.log.info(&format!("Would complete {} videos", total));
                let result = serde_json::json!({
                    "action": "complete_all",
                    "courses_scanned": courses.len(),
                    "total": total,
                    "completed": 0,
                    "failed": 0,
                    "dry_run": true,
                });
                format_and_output(&[result], ctx.output, None);
                return Ok(());
            }

            let config = load_config(cli.config.as_ref().and_then(|p| p.parent()));
            let launched = crate::auth::launch_persistent_session(&config, &ctx.log, true).await?;

            let mut completed = 0;
            for (cname, v) in &all_incomplete {
                ctx.log.info(&format!("Processing [{}]: {}", cname, v.name));

                let metadata = match get_video_metadata_browser(&launched.page, &v.url, &ctx.log).await {
                    Ok(m) => m,
                    Err(e) => {
                        ctx.log.warn(&format!("  Failed to get metadata: {}", e));
                        continue;
                    }
                };

                match complete_video(&ctx, &v, &metadata).await {
                    Ok(()) => {
                        ctx.log.success(&format!("  Completed: {}", v.name));
                        completed += 1;
                    }
                    Err(e) => {
                        ctx.log.warn(&format!("  Failed to complete: {} — {}", v.name, e));
                    }
                }
            }

            crate::auth::close_persistent_session(launched).await;
            ctx.log.info(&format!("Completed {}/{} videos", completed, total));
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

        crate::VideosCommands::Download { course_id, output_dir, cmid } => {
            let target_str = cmid.to_string();

            let videos = if let Some(cid) = course_id {
                get_supervideos_in_course_api(&ctx.client, &ctx.session, *cid).await?
            } else {
                let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;
                ctx.log.info(&format!("Scanning {} courses for cmid={}...", courses.len(), cmid));
                let mut found = Vec::new();
                for c in &courses {
                    if let Ok(vs) = get_supervideos_in_course_api(&ctx.client, &ctx.session, c.id).await {
                        for v in vs {
                            if v.cmid == target_str {
                                found.push(v);
                            }
                        }
                    }
                }
                found
            };

            if videos.is_empty() {
                ctx.log.info(&format!("No video found with cmid={}", cmid));
                return Ok(());
            }

            download_videos(&ctx, cli, videos, output_dir).await?;
        }

        crate::VideosCommands::DownloadAll { course_id, output_dir, incomplete_only } => {
            let mut videos = get_supervideos_in_course_api(&ctx.client, &ctx.session, *course_id).await?;
            if *incomplete_only {
                videos.retain(|v| !v.is_complete);
            }

            if videos.is_empty() {
                ctx.log.info("No videos to download.");
                return Ok(());
            }

            download_videos(&ctx, cli, videos, output_dir).await?;
        }
    }

    Ok(())
}

async fn download_videos(ctx: &ApiCtx, cli: &Cli, videos: Vec<SuperVideoModule>, output_dir: &std::path::PathBuf) -> Result<()> {
    ctx.log.info(&format!("Found {} videos", videos.len()));
    tokio::fs::create_dir_all(output_dir).await?;

    let config = load_config(cli.config.as_ref().and_then(|p| p.parent()));
    let launched = crate::auth::launch_persistent_session(&config, &ctx.log, true).await?;

    let cookies = match crate::auth::get_cookies(&launched.page).await {
        Ok(c) => c,
        Err(e) => {
            crate::auth::close_persistent_session(launched).await;
            anyhow::bail!("Failed to extract cookies: {}", e);
        }
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut downloaded = 0usize;
    let mut failed = 0usize;

    for v in &videos {
        ctx.log.info(&format!("Processing: {}", v.name));

        let metadata = match get_video_metadata_browser(&launched.page, &v.url, &ctx.log).await {
            Ok(m) => m,
            Err(e) => {
                ctx.log.warn(&format!("  Failed to get metadata: {}", e));
                results.push(serde_json::json!({
                    "name": v.name, "success": false, "error": e.to_string(),
                }));
                failed += 1;
                continue;
            }
        };

        let direct_url = metadata.video_sources.iter().find(|s| {
            s.contains("pluginfile.php") || s.ends_with(".mp4") || s.ends_with(".webm")
        });

        if let Some(url) = direct_url {
            let filename = sanitize_filename(&v.name, 200);
            let output_path = output_dir.join(format!("{}.mp4", filename));
            let path_str = output_path.to_string_lossy().to_string();

            match download_video_with_cookies(&cookies, url, &path_str, &ctx.log).await {
                Ok(size) => {
                    ctx.log.success(&format!("  Downloaded: {} ({:.1} KB)",
                        v.name, size as f64 / 1024.0));
                    results.push(serde_json::json!({
                        "name": v.name, "success": true, "path": path_str,
                        "type": "direct", "size": size,
                    }));
                    downloaded += 1;
                }
                Err(e) => {
                    ctx.log.warn(&format!("  Download failed: {}", e));
                    results.push(serde_json::json!({
                        "name": v.name, "success": false, "error": e.to_string(),
                    }));
                    failed += 1;
                }
            }
        } else if !metadata.youtube_ids.is_empty() {
            let yt_url = format!("https://www.youtube.com/watch?v={}", metadata.youtube_ids[0]);
            ctx.log.warn(&format!("  YouTube video: {}", yt_url));
            ctx.log.info("  Use yt-dlp to download YouTube videos.");
            results.push(serde_json::json!({
                "name": v.name, "success": false,
                "error": format!("YouTube video — use yt-dlp: yt-dlp {}", yt_url),
                "type": "youtube",
            }));
            failed += 1;
        } else {
            ctx.log.warn("  No downloadable video source found (embedded/blob URL).");
            results.push(serde_json::json!({
                "name": v.name, "success": false,
                "error": "No downloadable video source found",
                "type": "embedded",
            }));
            failed += 1;
        }
    }

    crate::auth::close_persistent_session(launched).await;

    ctx.log.info(&format!("\nResult: {} downloaded, {} failed", downloaded, failed));
    format_and_output(&results, ctx.output, None);
    Ok(())
}

/// Try to mark a video as complete via supervideo WS API, falling back to completion status.
async fn complete_video(ctx: &ApiCtx, v: &SuperVideoModule, metadata: &VideoMetadata) -> anyhow::Result<()> {
    ctx.log.debug(&format!(
        "  metadata: view_id={:?}, duration={:?}, sources={}, yt_ids={}",
        metadata.view_id, metadata.duration, metadata.video_sources.len(), metadata.youtube_ids.len()
    ));

    if let Some(view_id) = metadata.view_id {
        let duration = metadata.duration;
        let ok = save_video_progress_api(&ctx.client, &ctx.session, view_id, duration)
            .await
            .map_err(|e| anyhow::anyhow!("save_progress (view_id={}): {}", view_id, e))?;
        if !ok {
            anyhow::bail!("save_progress returned success=false (view_id={}, duration={})", view_id, duration);
        }
    } else {
        ctx.log.warn(&format!(
            "  No view_id/duration in page metadata, falling back to completion status API (cmid={})",
            v.cmid
        ));
        let cmid: u64 = v.cmid.parse().map_err(|_| anyhow::anyhow!("invalid cmid: {}", v.cmid))?;
        if cmid == 0 {
            anyhow::bail!("cmid is 0, cannot complete via fallback API");
        }
        let ok = update_completion_status(&ctx.client, &ctx.session, cmid, true)
            .await
            .map_err(|e| anyhow::anyhow!("update_completion (cmid={}): {}", cmid, e))?;
        if !ok {
            anyhow::bail!("update_completion returned status=false (cmid={})", cmid);
        }
    }
    Ok(())
}
