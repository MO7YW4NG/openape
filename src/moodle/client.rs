use crate::error::MoodleError;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

/// Build URL search params with Moodle's array encoding:
/// `courseids[0]=1&courseids[1]=2` and `options[0][name]=x&options[0][value]=y`.
/// Also handles nested objects for Moodle single_structure params:
/// `events[courseids][0]=1&events[courseids][1]=2` and `options[timestart]=123`.
pub fn build_ws_params(args: &HashMap<String, Value>) -> Vec<(String, String)> {
    let mut params = Vec::new();
    for (key, value) in args {
        push_ws_param(&mut params, key.clone(), value);
    }
    params
}

fn push_ws_param(params: &mut Vec<(String, String)>, key: String, value: &Value) {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                push_ws_param(params, format!("{key}[{index}]"), value);
            }
        }
        Value::Object(values) => {
            for (name, value) in values {
                push_ws_param(params, format!("{key}[{name}]"), value);
            }
        }
        Value::Null => {}
        Value::String(value) => params.push((key, value.clone())),
        value => params.push((key, value.to_string())),
    }
}

fn build_ws_request(
    client: &Client,
    base_url: &str,
    ws_token: &str,
    function: &str,
    args: &HashMap<String, Value>,
) -> Result<reqwest::Request, reqwest::Error> {
    client
        .get(format!("{base_url}/webservice/rest/server.php"))
        .query(&[
            ("wstoken", ws_token),
            ("wsfunction", function),
            ("moodlewsrestformat", "json"),
        ])
        .query(&build_ws_params(args))
        .build()
}

async fn check_ws_response(result: Value, function: &str) -> Result<Value, MoodleError> {
    if result.get("exception").is_some() || result.get("errorcode").is_some() {
        let msg = result
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| result.get("errorcode").and_then(|m| m.as_str()))
            .or_else(|| result.get("exception").and_then(|m| m.as_str()))
            .unwrap_or("Unknown error");
        return Err(MoodleError::WsApi {
            function: function.to_string(),
            message: msg.to_string(),
        });
    }
    if result
        .get("error")
        .and_then(|e| e.as_bool())
        .unwrap_or(false)
    {
        let msg = result
            .get("message")
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
    let request = build_ws_request(client, base_url, ws_token, function, args)?;
    let result: Value = client.execute(request).await?.json().await?;
    check_ws_response(result, function).await
}

/// Moodle Web Service call with an SEB ConfigKeyHash computed from the encoded URL.
pub async fn moodle_api_call_seb(
    client: &Client,
    base_url: &str,
    ws_token: &str,
    function: &str,
    args: &HashMap<String, Value>,
    seb_config_key: &str,
) -> Result<Value, MoodleError> {
    use crate::moodle::seb::compute_config_key_hash;
    use reqwest::header::{HeaderName, HeaderValue};

    let mut request = build_ws_request(client, base_url, ws_token, function, args)?;
    let hash = compute_config_key_hash(request.url().as_str(), seb_config_key);
    request.headers_mut().insert(
        HeaderName::from_static("x-safeexambrowser-configkeyhash"),
        HeaderValue::from_bytes(hash.as_bytes()).expect("SHA-256 hex is a valid HTTP header value"),
    );
    let result: Value = client.execute(request).await?.json().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ws_params_percent_encode_values_and_preserve_nested_keys() {
        let value = "one&forged=two+three#four%五";
        let args = HashMap::from([
            ("courseids".to_string(), json!([value])),
            (
                "options".to_string(),
                json!([{"name": value, "value": value}]),
            ),
            (
                "events".to_string(),
                json!({"courseids": [value], "filter": {"name": value}}),
            ),
        ]);

        let request = build_ws_request(
            &Client::new(),
            "https://example.com",
            "token",
            "test_function",
            &args,
        )
        .unwrap();
        let params: HashMap<_, _> = request.url().query_pairs().into_owned().collect();

        assert_eq!(params.len(), 8);
        for key in [
            "courseids[0]",
            "options[0][name]",
            "options[0][value]",
            "events[courseids][0]",
            "events[filter][name]",
        ] {
            assert_eq!(params.get(key).map(String::as_str), Some(value));
        }
        assert!(!params.contains_key("forged"));
    }
}
