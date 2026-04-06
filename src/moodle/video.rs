use super::client::moodle_api_call;
use crate::moodle_args;
use super::course::get_site_info;
use super::types::{SessionInfo, SuperVideoModule};
use reqwest::Client;
use std::collections::HashMap;

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
