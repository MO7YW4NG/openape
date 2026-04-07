use super::client::moodle_api_call;
use crate::moodle_args;
use crate::utils::strip_html_tags;
use super::types::{PageModule, SessionInfo};
use reqwest::Client;
use serde_json::Value;

/// Get page modules by course IDs via WS API.
pub async fn get_pages_by_courses_api(
    client: &Client,
    session: &SessionInfo,
    course_ids: &[u64],
) -> anyhow::Result<Vec<PageModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    if course_ids.is_empty() {
        return Ok(Vec::new());
    }

    let course_ids_json: Vec<Value> = course_ids.iter().map(|id| serde_json::json!(*id)).collect();
    let args = moodle_args!("courseids" => course_ids_json);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_page_get_pages_by_courses", &args).await?;

    let pages = data.get("pages").and_then(|p| p.as_array()).cloned().unwrap_or_default();

    Ok(pages.into_iter().map(|p| {
        PageModule {
            id: p.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            cmid: p.get("coursemodule").and_then(|v| v.as_u64())
                .or_else(|| p.get("cmid").and_then(|v| v.as_u64()))
                .unwrap_or(0).to_string(),
            name: p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            course_id: p.get("course").and_then(|v| v.as_u64()).unwrap_or(0),
            content: p.get("content").and_then(|v| v.as_str()).map(strip_html_tags),
            timemodified: p.get("timemodified").and_then(|v| v.as_i64()),
        }
    }).collect())
}
