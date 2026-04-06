use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{Message, SessionInfo};
use reqwest::Client;

/// Get messages via WS API.
pub async fn get_messages_api(
    client: &Client,
    session: &SessionInfo,
    user_id_to: u64,
    user_id_from: Option<u64>,
    read: Option<bool>,
    limit_num: Option<u32>,
) -> anyhow::Result<Vec<Message>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let mut args = moodle_args!("useridto" => user_id_to);
    if let Some(from) = user_id_from { args.insert("useridfrom".to_string(), serde_json::json!(from)); }
    if let Some(r) = read { args.insert("read".to_string(), serde_json::json!(r)); }
    if let Some(limit) = limit_num { args.insert("limitnum".to_string(), serde_json::json!(limit)); }

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_message_get_messages", &args).await?;

    let messages = data.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    Ok(messages.into_iter().map(|m| {
        Message {
            id: m.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            useridfrom: m.get("useridfrom").and_then(|v| v.as_u64()).unwrap_or(0),
            useridto: m.get("useridto").and_then(|v| v.as_u64()).unwrap_or(0),
            subject: m.get("subject").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            text: m.get("smallmessage").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            timecreated: m.get("timecreated").and_then(|v| v.as_i64()).unwrap_or(0),
        }
    }).collect())
}
