use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Stored credentials for headless re-login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub id: String,
    #[serde(
        serialize_with = "serialize_password",
        deserialize_with = "deserialize_password"
    )]
    pub password: String,
}

fn serialize_password<S: serde::Serializer>(pw: &str, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&STANDARD.encode(pw.as_bytes()))
}

fn deserialize_password<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let encoded = String::deserialize(d)?;
    STANDARD
        .decode(&encoded)
        .map_err(|e| serde::de::Error::custom(format!("invalid base64: {e}")))
        .and_then(|bytes| {
            String::from_utf8(bytes)
                .map_err(|e| serde::de::Error::custom(format!("invalid utf8: {e}")))
        })
}

impl StoredCredentials {
    pub fn email(&self) -> String {
        format!("{}@o365st.cycu.edu.tw", self.id)
    }

    pub fn path(auth_state_path: &str) -> PathBuf {
        let path = Path::new(auth_state_path);
        let dir = path.parent().unwrap_or(Path::new(".auth"));
        dir.join("credentials.json")
    }

    pub fn load(auth_state_path: &str) -> Option<Self> {
        let path = Self::path(auth_state_path);
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, auth_state_path: &str) {
        let path = Self::path(auth_state_path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn delete(auth_state_path: &str) {
        let path = Self::path(auth_state_path);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}
