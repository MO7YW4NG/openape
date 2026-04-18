use super::client::moodle_api_call;
use super::types::{CalendarEvent, SessionInfo};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

/// Get calendar events via WS API.
pub async fn get_calendar_events_api(
    client: &Client,
    session: &SessionInfo,
    course_id: Option<u64>,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let mut args = HashMap::new();

    // Moodle core_calendar_get_calendar_events expects nested structure:
    // events[courseids][0]=X, options[timestart]=Y, options[timeend]=Z
    let mut events_obj = serde_json::Map::new();
    if let Some(cid) = course_id {
        events_obj.insert("courseids".to_string(), serde_json::json!([cid]));
    }
    let mut options_obj = serde_json::Map::new();
    if let Some(st) = start_time { options_obj.insert("timestart".to_string(), serde_json::json!(st)); }
    if let Some(et) = end_time { options_obj.insert("timeend".to_string(), serde_json::json!(et)); }

    args.insert("events".to_string(), Value::Object(events_obj));
    if !options_obj.is_empty() {
        args.insert("options".to_string(), Value::Object(options_obj));
    }

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_calendar_get_calendar_events", &args).await?;

    let events = data.get("events").and_then(|e| e.as_array()).cloned().unwrap_or_default();

    let filtered: Vec<_> = events.into_iter().filter(|e| {
        if let Some(et) = end_time {
            let timestart = e.get("timestart").and_then(|v| v.as_i64()).unwrap_or(0);
            timestart <= et
        } else {
            true
        }
    }).collect();

    Ok(filtered.into_iter().map(|e| {
        CalendarEvent {
            id: e.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: e.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            description: e.get("description").and_then(|v| v.as_str()).map(String::from),
            format: e.get("format").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            courseid: e.get("courseid").and_then(|v| v.as_u64()),
            categoryid: e.get("categoryid").and_then(|v| v.as_u64()),
            groupid: e.get("groupid").and_then(|v| v.as_u64()),
            userid: e.get("userid").and_then(|v| v.as_u64()),
            moduleid: e.get("moduleid").and_then(|v| v.as_u64()),
            modulename: e.get("modulename").and_then(|v| v.as_str()).map(String::from),
            instance: e.get("instance").and_then(|v| v.as_u64()),
            eventtype: e.get("eventtype").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            timestart: e.get("timestart").and_then(|v| v.as_i64()).unwrap_or(0),
            timeduration: e.get("timeduration").and_then(|v| v.as_i64()).filter(|&d| d != 0),
            timedue: e.get("timedue").and_then(|v| v.as_i64()).filter(|&d| d != 0),
            visible: e.get("visible").and_then(|v| v.as_i64()).map(|v| v as u32),
            location: e.get("location").and_then(|v| v.as_str()).map(String::from),
        }
    }).collect())
}
