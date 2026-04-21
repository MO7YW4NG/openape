use crate::error::MoodleError;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

/// Build URL search params with Moodle's array encoding:
/// `courseids[0]=1&courseids[1]=2` and `options[0][name]=x&options[0][value]=y`.
/// Also handles nested objects for Moodle single_structure params:
/// `events[courseids][0]=1&events[courseids][1]=2` and `options[timestart]=123`.
pub fn build_ws_params(args: &HashMap<String, Value>) -> String {
    let mut parts = Vec::new();

    for (key, value) in args {
        match value {
            Value::Array(arr) if key == "options" => {
                for (i, opt) in arr.iter().enumerate() {
                    if let (Some(name), Some(val)) = (opt.get("name"), opt.get("value")) {
                        parts.push(format!("{}[{}][name]={}", key, i, name));
                        parts.push(format!("{}[{}][value]={}", key, i, val));
                    }
                }
            }
            Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    parts.push(format!("{}[{}]={}", key, i, s));
                }
            }
            Value::Object(obj) => {
                // Nested object (e.g., events[courseids][0]=1, options[timestart]=123)
                for (sub_key, sub_val) in obj {
                    match sub_val {
                        Value::Array(arr) => {
                            for (i, v) in arr.iter().enumerate() {
                                let s = match v {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                parts.push(format!("{}[{}][{}]={}", key, sub_key, i, s));
                            }
                        }
                        Value::Null => {}
                        other => {
                            parts.push(format!("{}[{}]={}", key, sub_key, other));
                        }
                    }
                }
            }
            Value::Null => {}
            Value::String(s) => {
                parts.push(format!("{}={}", key, s));
            }
            other => {
                parts.push(format!("{}={}", key, other));
            }
        }
    }

    parts.join("&")
}

fn build_ws_url(base_url: &str, ws_token: &str, function: &str, args: &HashMap<String, Value>) -> String {
    let params = build_ws_params(args);
    format!(
        "{}/webservice/rest/server.php?wstoken={}&wsfunction={}&moodlewsrestformat=json&{}",
        base_url, ws_token, function, params
    )
}

async fn check_ws_response(result: Value, function: &str) -> Result<Value, MoodleError> {
    if result.get("exception").is_some() || result.get("errorcode").is_some() {
        let msg = result.get("message")
            .and_then(|m| m.as_str())
            .or_else(|| result.get("errorcode").and_then(|m| m.as_str()))
            .or_else(|| result.get("exception").and_then(|m| m.as_str()))
            .unwrap_or("Unknown error");
        return Err(MoodleError::WsApi {
            function: function.to_string(),
            message: msg.to_string(),
        });
    }
    if result.get("error").and_then(|e| e.as_bool()).unwrap_or(false) {
        let msg = result.get("message")
            .or_else(|| result.get("exception").and_then(|e| e.get("message")))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        return Err(MoodleError::WsApi {
            function: function.to_string(),
            message: msg.to_string(),
        });
    }
    Ok(result)
}

/// Direct HTTP API call via Moodle Web Service REST endpoint (no browser).
pub async fn moodle_api_call(
    client: &Client,
    base_url: &str,
    ws_token: &str,
    function: &str,
    args: &HashMap<String, Value>,
) -> Result<Value, MoodleError> {
    let url = build_ws_url(base_url, ws_token, function, args);
    let result: Value = client.get(&url).send().await?.json().await?;
    check_ws_response(result, function).await
}

/// Like `moodle_api_call` but injects the SEB ConfigKeyHash header.
/// Hash is computed as SHA256(requestURL + config_key), matching SEB browser behavior.
pub async fn moodle_api_call_seb(
    client: &Client,
    base_url: &str,
    ws_token: &str,
    function: &str,
    args: &HashMap<String, Value>,
    seb_config_key: &str,
) -> Result<Value, MoodleError> {
    use crate::moodle::seb::compute_config_key_hash;
    let url = build_ws_url(base_url, ws_token, function, args);
    let hash = compute_config_key_hash(&url, seb_config_key);
    let result: Value = client.get(&url)
        .header("X-SafeExamBrowser-ConfigKeyHash", hash)
        .send().await?
        .json().await?;
    check_ws_response(result, function).await
}

/// Helper to build args HashMap quickly.
#[macro_export]
macro_rules! moodle_args {
    ($($key:expr => $val:expr),* $(,)?) => {{
        let mut m = std::collections::HashMap::new();
        $(
            m.insert($key.to_string(), serde_json::json!($val));
        )*
        m
    }};
}
