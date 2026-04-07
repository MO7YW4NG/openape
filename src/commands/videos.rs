use anyhow::Result;
use crate::Cli;
use crate::config::load_config;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::video::{get_supervideos_in_course_api, get_incomplete_videos_api, update_completion_status, get_video_metadata_browser, download_video_with_cookies, save_video_progress_api};
use crate::output::format_and_output;
use crate::utils::sanitize_filename;
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
            let launched = crate::auth::launch_persistent_session(&config, &ctx.log).await?;

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

                // 2. Complete via API if view_id/duration found
                let success = if let (Some(view_id), Some(duration)) = (metadata.view_id, metadata.duration) {
                    save_video_progress_api(&ctx.client, &ctx.session, view_id, duration).await.unwrap_or(false)
                } else {
                    // Fallback to manual completion if possible
                    let cmid: u64 = v.cmid.parse().unwrap_or(0);
                    if cmid != 0 {
                        update_completion_status(&ctx.client, &ctx.session, cmid, true).await.unwrap_or(false)
                    } else {
                        false
                    }
                };

                if success {
                    ctx.log.success(&format!("  Completed: {}", v.name));
                    completed += 1;
                } else {
                    ctx.log.warn(&format!("  Failed to complete: {}", v.name));
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
            let launched = crate::auth::launch_persistent_session(&config, &ctx.log).await?;

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

                let success = if let (Some(view_id), Some(duration)) = (metadata.view_id, metadata.duration) {
                    save_video_progress_api(&ctx.client, &ctx.session, view_id, duration).await.unwrap_or(false)
                } else {
                    let cmid: u64 = v.cmid.parse().unwrap_or(0);
                    if cmid != 0 {
                        update_completion_status(&ctx.client, &ctx.session, cmid, true).await.unwrap_or(false)
                    } else {
                        false
                    }
                };

                if success {
                    ctx.log.success(&format!("  Completed: {}", v.name));
                    completed += 1;
                } else {
                    ctx.log.warn(&format!("  Failed to complete: {}", v.name));
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

        crate::VideosCommands::Download { course_id, output_dir, incomplete_only } => {
            // Phase 1: List videos via API (fast, no browser needed)
            let mut videos = get_supervideos_in_course_api(&ctx.client, &ctx.session, *course_id).await?;
            if *incomplete_only {
                videos.retain(|v| !v.is_complete);
            }
            if videos.is_empty() {
                ctx.log.info("No videos to download.");
                return Ok(());
            }

            ctx.log.info(&format!("Found {} videos", videos.len()));
            tokio::fs::create_dir_all(output_dir).await?;

            // Phase 2: Launch browser session for authenticated page access
            let config = load_config(cli.config.as_ref().and_then(|p| p.parent()));
            let launched = crate::auth::launch_persistent_session(&config, &ctx.log).await?;

            // Extract cookies once for all downloads
            let cookies = match crate::auth::get_cookies(&launched.page).await {
                Ok(c) => c,
                Err(e) => {
                    crate::auth::close_persistent_session(launched).await;
                    anyhow::bail!("Failed to extract cookies: {}", e);
                }
            };

            // Phase 3: Process each video
            let mut results: Vec<serde_json::Value> = Vec::new();
            let mut downloaded = 0usize;
            let mut failed = 0usize;

            for v in &videos {
                ctx.log.info(&format!("Processing: {}", v.name));

                // Extract metadata via browser navigation
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

                // Priority 1: Direct video URL (pluginfile.php, .mp4, .webm)
                let direct_url = metadata.video_sources.iter().find(|s| {
                    s.contains("pluginfile.php") || s.ends_with(".mp4") || s.ends_with(".webm")
                });

                if let Some(url) = direct_url {
                    let filename = sanitize_filename(&v.name, 200);
                    let output_path = std::path::Path::new(output_dir).join(format!("{}.mp4", filename));
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

            // Phase 4: Close browser
            crate::auth::close_persistent_session(launched).await;

            ctx.log.info(&format!("\nResult: {} downloaded, {} failed", downloaded, failed));
            format_and_output(&results, ctx.output, None);
        }
    }

    Ok(())
}
