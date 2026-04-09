use std::path::Path;

pub const MOODLE_BASE_URL: &str = "https://ilearning.cycu.edu.tw";

/// Application configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub moodle_base_url: String,
    pub headless: bool,
    pub auth_state_path: String,
}

/// Load configuration from .env file (if it exists) and environment variables.
pub fn load_config(base_dir: Option<&Path>) -> AppConfig {
    if let Some(dir) = base_dir {
        let env_path = dir.join(".env");
        if env_path.exists() {
            dotenvy::from_path(&env_path).ok();
        }
    } else {
        dotenvy::dotenv().ok();
    }

    AppConfig {
        moodle_base_url: MOODLE_BASE_URL.to_string(),
        headless: std::env::var("HEADLESS").unwrap_or_else(|_| "true".to_string()) != "false",
        auth_state_path: std::env::var("AUTH_STATE_PATH")
            .unwrap_or_else(|_| ".auth/storage-state.json".to_string()),
    }
}

/// Load config and apply CLI-level overrides.
pub fn load_config_for_cli(cli: &crate::Cli) -> AppConfig {
    let mut config = load_config(cli.config.as_ref().and_then(|p| p.parent()));

    // Keep parity with legacy CLI behavior where `--headed` forces non-headless mode.
    if cli.headed {
        config.headless = false;
    }

    // Always honor effective CLI session path (default or explicit override).
    config.auth_state_path = cli.session.to_string_lossy().to_string();

    config
}
