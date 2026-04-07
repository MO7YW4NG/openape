use super::client::moodle_api_call;
use crate::moodle_args;
use crate::auth::{Cookie, cookies_to_cookie_header};
use super::course::get_site_info;
use super::types::{SessionInfo, SuperVideoModule};
use crate::logger::Logger;
use reqwest::Client;
use std::collections::HashMap;
use chromiumoxide::Page;

/// Get supervideos in a course via WS API.
pub async fn get_supervideos_in_course_api(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
) -> anyhow::Result<Vec<SuperVideoModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    // Get course contents
    let args = moodle_args!("courseid" => course_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_course_get_contents", &args).await?;

    let sections = data.as_array().cloned().unwrap_or_default();
    let mut videos = Vec::new();

    for section in &sections {
        let modules = section.get("modules").and_then(|m| m.as_array()).cloned().unwrap_or_default();
        for module in &modules {
            let modname = module.get("modname").and_then(|v| v.as_str()).unwrap_or("");
            if modname == "supervideo" {
                videos.push(SuperVideoModule {
                    cmid: module.get("id").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
                    name: module.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    url: module.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    instance: module.get("instance").and_then(|v| v.as_u64()),
                    is_complete: false,
                });
            }
        }
    }

    // Get completion status
    if let Ok(site_info) = get_site_info(client, session).await {
        let comp_args = moodle_args!("courseid" => course_id, "userid" => site_info.userid);
        if let Ok(comp_data) = moodle_api_call(client, &session.moodle_base_url, ws_token,
            "core_completion_get_activities_completion_status", &comp_args).await
        {
            if let Some(statuses) = comp_data.get("statuses").and_then(|s| s.as_array()) {
                let completion_map: HashMap<u64, bool> = statuses.iter()
                    .filter_map(|s| {
                        let has_completion = s.get("hascompletion").and_then(|v| v.as_bool()).unwrap_or(false);
                        let cmid = s.get("cmid").and_then(|v| v.as_u64())?;
                        let is_complete = s.get("isoverallcomplete").and_then(|v| v.as_bool()).unwrap_or(false);
                        if has_completion { Some((cmid, is_complete)) } else { None }
                    })
                    .collect();

                for video in &mut videos {
                    if let Ok(cmid) = video.cmid.parse::<u64>() {
                        if let Some(&complete) = completion_map.get(&cmid) {
                            video.is_complete = complete;
                        }
                    }
                }
            }
        }
    }

    Ok(videos)
}

/// Get only incomplete videos with completion tracking.
pub async fn get_incomplete_videos_api(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
) -> anyhow::Result<Vec<SuperVideoModule>> {
    let all = get_supervideos_in_course_api(client, session, course_id).await?;
    Ok(all.into_iter().filter(|v| !v.is_complete).collect())
}

/// Build duration map array for video progress tracking.
fn build_duration_map(duration: u64) -> String {
    let mut map = Vec::new();
    for i in 0..100 {
        map.push(serde_json::json!({
            "time": (duration * i) / 100,
            "percent": i,
        }));
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "[]".to_string())
}

/// Complete a video via supervideo-specific WS API.
pub async fn save_video_progress_api(
    client: &Client,
    session: &SessionInfo,
    view_id: u64,
    duration: u64,
) -> anyhow::Result<bool> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    let progress_args = moodle_args!(
        "view_id" => view_id,
        "currenttime" => duration,
        "duration" => duration,
        "percent" => 100,
        "mapa" => build_duration_map(duration),
    );

    let result = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_supervideo_progress_save_mobile", &progress_args).await?;

    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false)
        || result.as_array().and_then(|a| a.first()).and_then(|v| v.get("success")).and_then(|v| v.as_bool()).unwrap_or(false);

    Ok(success)
}

/// Update activity completion status via WS API.
pub async fn update_completion_status(
    client: &Client,
    session: &SessionInfo,
    cmid: u64,
    completed: bool,
) -> anyhow::Result<bool> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("cmid" => cmid, "completed" => if completed { 1 } else { 0 });
    let result = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_completion_update_activity_completion_status_manually", &args).await?;
    
    Ok(result.get("status").and_then(|v| v.as_bool()).unwrap_or(false))
}

/// Resolved video metadata from a supervideo page.
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub video_sources: Vec<String>,
    pub youtube_ids: Vec<String>,
    pub view_id: Option<u64>,
    pub duration: Option<u64>,
}

/// Extract video sources and YouTube IDs from HTML content.
/// Shared by both API and browser-based metadata extraction.
fn extract_video_sources_from_html(html: &str, log: &Logger) -> anyhow::Result<VideoMetadata> {
    let mut video_sources: Vec<String> = Vec::new();
    let mut youtube_ids: Vec<String> = Vec::new();

    // 1. <source src="...">
    let source_re = regex::Regex::new(r#"<source[^>]+src=["']([^"']+)["']"#)?;
    for cap in source_re.captures_iter(html) {
        video_sources.push(cap[1].to_string());
    }

    // 2. <video src="...">
    let video_re = regex::Regex::new(r#"<video[^>]+src=["']([^"']+)["']"#)?;
    for cap in video_re.captures_iter(html) {
        video_sources.push(cap[1].to_string());
    }

    // 3. <iframe src="...">
    let iframe_re = regex::Regex::new(r#"<iframe[^>]+src=["']([^"']+)["']"#)?;
    for cap in iframe_re.captures_iter(html) {
        let src = &cap[1];
        video_sources.push(src.to_string());
        if let Some(yt_cap) = regex::Regex::new(
            r"(?:youtube\.com/(?:embed/|v/|watch\?v=)|youtu\.be/)([a-zA-Z0-9_-]{11})"
        )?.captures(src) {
            youtube_ids.push(yt_cap[1].to_string());
        }
    }

    // 4. Extract view_id and duration from JavaScript
    let view_id = regex::Regex::new(r#""view_id"\s*:\s*(\d+)"#)?
        .captures(html)
        .and_then(|c| c[1].parse::<u64>().ok());
    
    let duration = regex::Regex::new(r#""duration"\s*:\s*(\d+)"#)?
        .captures(html)
        .and_then(|c| c[1].parse::<u64>().ok());

    video_sources.dedup();

    log.debug(&format!("Found {} video source(s)", video_sources.len()));
    if let Some(vid) = view_id { log.debug(&format!("  View ID: {}", vid)); }
    if let Some(dur) = duration { log.debug(&format!("  Duration: {}", dur)); }

    Ok(VideoMetadata { video_sources, youtube_ids, view_id, duration })
}

/// Extract video metadata from a supervideo page using an authenticated browser.
pub async fn get_video_metadata_browser(
    page: &Page,
    activity_url: &str,
    log: &Logger,
) -> anyhow::Result<VideoMetadata> {
    log.debug(&format!("Navigating to video page: {}", activity_url));

    page.goto(activity_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to navigate to video page: {}", e))?;

    // Wait for page content to load
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = page.url()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get page URL: {}", e))?;
    if let Some(url_str) = url {
        if url_str.contains("login") || url_str.contains("microsoftonline") {
            anyhow::bail!("Session invalid — redirected to login page");
        }
    }

    let html = page.content()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get page content: {}", e))?;

    extract_video_sources_from_html(&html, log)
}

/// Download a video file using cookies extracted from a browser session.
pub async fn download_video_with_cookies(
    cookies: &[Cookie],
    video_url: &str,
    output_path: &str,
    log: &Logger,
) -> anyhow::Result<u64> {
    let cookie_header = cookies_to_cookie_header(cookies, video_url);

    if cookie_header.is_empty() {
        anyhow::bail!("No relevant cookies found for download URL");
    }

    let cookie_count = cookie_header.split("; ").count();
    log.debug(&format!("Downloading with {} cookie(s)", cookie_count));

    let client = Client::builder()
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

    let resp = client
        .get(video_url)
        .header("Cookie", &cookie_header)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Download request failed: {}", e))?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} — download failed", resp.status());
    }

    let bytes = resp.bytes().await
        .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

    tokio::fs::write(output_path, &bytes).await
        .map_err(|e| anyhow::anyhow!("Failed to write file: {}", e))?;

    Ok(bytes.len() as u64)
}
