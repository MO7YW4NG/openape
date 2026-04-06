use super::client::moodle_api_call;
use crate::utils::extract_course_name;
use super::types::{EnrolledCourse, SessionInfo, SiteInfo};
use reqwest::Client;
use std::collections::HashMap;

/// Fetch enrolled courses via pure API (no browser).
pub async fn get_enrolled_courses_api(
    client: &Client,
    session: &SessionInfo,
    classification: &str,
) -> anyhow::Result<Vec<EnrolledCourse>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    let mut args = HashMap::new();
    args.insert("offset".to_string(), serde_json::json!(0));
    args.insert("limit".to_string(), serde_json::json!(0));
    args.insert("classification".to_string(), serde_json::json!(classification));
    args.insert("sort".to_string(), serde_json::json!("fullname"));
    args.insert("customfieldname".to_string(), serde_json::json!(""));
    args.insert("customfieldvalue".to_string(), serde_json::json!(""));

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_course_get_enrolled_courses_by_timeline_classification", &args).await?;

    let courses = data.get("courses")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(courses.into_iter().map(|c| {
        EnrolledCourse {
            id: c.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            fullname: extract_course_name(c.get("fullname").and_then(|v| v.as_str()).unwrap_or("")),
            shortname: c.get("shortname").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            idnumber: c.get("idnumber").and_then(|v| v.as_str()).map(|s| s.to_string()),
            category: c.get("category").and_then(|cat| cat.get("name")).and_then(|n| n.as_str()).map(|s| s.to_string()),
            progress: c.get("progress").and_then(|v| v.as_u64()).map(|p| p as u32),
            startdate: c.get("startdate").and_then(|v| v.as_i64()),
            enddate: c.get("enddate").and_then(|v| v.as_i64()),
        }
    }).collect())
}

/// Get site info including current user ID.
pub async fn get_site_info(
    client: &Client,
    session: &SessionInfo,
) -> anyhow::Result<SiteInfo> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = HashMap::new();
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_webservice_get_site_info", &args).await?;

    Ok(SiteInfo {
        userid: data.get("userid").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

/// Calculate Moodle user context ID.
pub fn get_user_context_id(user_id: u64) -> u64 {
    user_id * 10 + 30
}
