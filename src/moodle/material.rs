use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{ResourceModule, SessionInfo};
use reqwest::Client;
use serde_json::Value;

/// Get resources by course IDs via WS API.
pub async fn get_resources_by_courses_api(
    client: &Client,
    session: &SessionInfo,
    course_ids: &[u64],
) -> anyhow::Result<Vec<ResourceModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    if course_ids.is_empty() {
        return Ok(Vec::new());
    }

    let course_ids_json: Vec<Value> = course_ids.iter().map(|id| serde_json::json!(*id)).collect();
    let args = moodle_args!("courseids" => course_ids_json);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_resource_get_resources_by_courses", &args).await?;

    let resources = data.get("resources").and_then(|r| r.as_array()).cloned().unwrap_or_default();

    Ok(resources.into_iter().map(|r| {
        let first_file = r.get("contentfiles").and_then(|f| f.as_array()).and_then(|arr| arr.first());
        ResourceModule {
            cmid: r.get("coursemodule").and_then(|v| v.as_u64())
                .or_else(|| r.get("id").and_then(|v| v.as_u64()))
                .unwrap_or(0).to_string(),
            name: r.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            url: first_file.and_then(|f| f.get("fileurl")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            course_id: r.get("course").and_then(|v| v.as_u64()).unwrap_or(0),
            mod_type: "resource".to_string(),
            mimetype: first_file.and_then(|f| f.get("mimetype")).and_then(|v| v.as_str()).map(String::from),
            filesize: first_file.and_then(|f| f.get("filesize")).and_then(|v| v.as_u64()),
            modified: r.get("timemodified").and_then(|v| v.as_i64()),
        }
    }).collect())
}

/// Mark a resource as viewed via WS API (for completionview-type tracking).
pub async fn view_resource_api(
    client: &Client,
    session: &SessionInfo,
    instance_id: u64,
) -> anyhow::Result<bool> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("resourceid" => instance_id);
    let result = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_resource_view_resource", &args).await?;

    Ok(result.get("status").and_then(|v| v.as_bool()).unwrap_or(result.is_null()))
}

/// Get incomplete activity completion statuses for a course.
pub async fn get_incomplete_completions(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
    userid: u64,
) -> anyhow::Result<Vec<IncompleteCompletion>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("courseid" => course_id, "userid" => userid);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_completion_get_activities_completion_status", &args).await?;

    let statuses = data.get("statuses").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    Ok(statuses.into_iter().filter_map(|s| {
        let hascompletion = s.get("hascompletion").and_then(|v| v.as_bool()).unwrap_or(false);
        let overall = s.get("isoverallcomplete").and_then(|v| v.as_bool()).unwrap_or(false);
        if !hascompletion || overall { return None; }

        let cmid = s.get("cmid").and_then(|v| v.as_u64()).unwrap_or(0);
        let instance = s.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
        let modname = s.get("modname").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // Extract completion rule
        let rule = s.get("details").and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|d| d.get("rulename").and_then(|v| v.as_str()))
            .map(String::from);

        Some(IncompleteCompletion { cmid, instance, modname, name, rule })
    }).collect())
}

/// Info about an incomplete activity's completion tracking.
pub struct IncompleteCompletion {
    pub cmid: u64,
    pub instance: u64,
    pub modname: String,
    pub name: String,
    pub rule: Option<String>,
}
