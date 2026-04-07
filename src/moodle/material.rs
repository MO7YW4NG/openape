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
