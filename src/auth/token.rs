use serde::{Deserialize, Serialize};
use std::path::Path;

/// Cached session metadata (sesskey + WS token with expiration).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMeta {
    pub sesskey: Option<String>,
    pub sesskey_timestamp: Option<i64>,
    pub ws_token: Option<String>,
    pub ws_token_timestamp: Option<i64>,
    pub user_agent: Option<String>,
    pub seb_config_key: Option<String>,
}

impl SessionMeta {
    pub fn load(auth_state_path: &str) -> Self {
        let meta_path = Self::meta_path(auth_state_path);
        if Path::new(&meta_path).exists() {
            if let Ok(content) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str(&content) {
                    return meta;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self, auth_state_path: &str) {
        let meta_path = Self::meta_path(auth_state_path);
        if let Some(parent) = Path::new(&meta_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(self).unwrap_or_default());
    }

    pub(crate) fn meta_path(auth_state_path: &str) -> String {
        let path = Path::new(auth_state_path);
        let dir = path.parent().unwrap_or(Path::new(".auth"));
        dir.join("session-meta.json").to_string_lossy().to_string()
    }

    /// Load sesskey if cached and not expired (6 hours).
    pub fn get_sesskey(&self) -> Option<String> {
        if let (Some(ref key), Some(ts)) = (&self.sesskey, self.sesskey_timestamp) {
            let age = chrono::Utc::now().timestamp() - ts;
            if age < 6 * 3600 {
                return Some(key.clone());
            }
        }
        None
    }

    /// Load WS token if cached and not expired (24 hours).
    pub fn get_ws_token(&self) -> Option<String> {
        if let (Some(ref token), Some(ts)) = (&self.ws_token, self.ws_token_timestamp) {
            let age = chrono::Utc::now().timestamp() - ts;
            if age < 24 * 3600 {
                return Some(token.clone());
            }
        }
        None
    }

    /// Save WS token with current timestamp.
    pub fn set_ws_token(&mut self, token: &str) {
        self.ws_token = Some(token.to_string());
        self.ws_token_timestamp = Some(chrono::Utc::now().timestamp());
    }

    /// Save user agent string.
    pub fn set_user_agent(&mut self, ua: &str) {
        self.user_agent = Some(ua.to_string());
    }


}

/// Extract and decode the WS token from a moodlemobile:// URL.
/// Format: moodlemobile://token=BASE64_DATA
/// Decoded: token:::site_url:::other_params
pub fn extract_token_from_custom_scheme(url: &str) -> Option<String> {
    let re = regex::Regex::new(r"token=([A-Za-z0-9+/=]+)").ok()?;
    let caps = re.captures(url)?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(&caps[1]).ok()?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    let parts: Vec<&str> = decoded_str.split(":::").collect();
    parts.get(1).map(|s| s.to_string())
}
