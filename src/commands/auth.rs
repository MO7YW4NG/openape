use anyhow::Result;
use crate::Cli;
use crate::config::load_config_for_cli;
use crate::logger::Logger;
use crate::auth;
use crate::output::format_and_output;
use std::path::Path;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const NPM_PACKAGE_NAME: &str = "@mo7yw4ng/openape";
const NPM_REGISTRY_API: &str = "https://registry.npmjs.org/@mo7yw4ng%2fopenape";

async fn check_for_update(log: &Logger) {
    let client = reqwest::Client::builder()
        .user_agent(format!("openape/{}", CURRENT_VERSION))
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let Ok(client) = client else { return };

    let Ok(resp) = client.get(NPM_REGISTRY_API).send().await else { return };
    let Ok(json) = resp.json::<serde_json::Value>().await else { return };

    if json.get("error").is_some() {
        log.debug("Update check skipped: package not found on npm registry.");
        return;
    }

    if let Some(latest) = json["dist-tags"]["latest"].as_str() {
        if latest != CURRENT_VERSION {
            log.warn(&format!(
                "Update available on npm: v{} -> v{}  (run: npm install -g {}@latest)",
                CURRENT_VERSION, latest, NPM_PACKAGE_NAME
            ));
        }
    }
}

pub async fn run(cmd: &crate::AuthCommands, cli: &Cli) -> Result<()> {
    let config = load_config_for_cli(cli);
    let log = Logger::new(cli.verbose, cli.silent);

    match cmd {
        crate::AuthCommands::Login => {
            check_for_update(&log).await;
            log.info("Launching browser for login...");
            let (launched, ws_token) = auth::launch_authenticated(&config, &log).await?;
            // Close browser after login
            auth::close_persistent_session(launched).await;
            
            match ws_token {
                Some(token) => {
                    log.success("Login successful!");
                    log.info(&format!("WS Token: {}...", &token[..token.len().min(20)]));
                    let result = serde_json::json!({
                        "action": "login",
                        "success": true,
                        "ws_token_prefix": &token[..token.len().min(20)],
                        "version": CURRENT_VERSION,
                    });
                    format_and_output(&[result], cli.output, None);
                }
                None => {
                    log.warn("Logged in but could not acquire WS token. Some commands may not work.");
                    let result = serde_json::json!({
                        "action": "login",
                        "success": true,
                        "ws_token_prefix": null,
                        "warning": "Could not acquire WS token",
                        "version": CURRENT_VERSION,
                    });
                    format_and_output(&[result], cli.output, None);
                }
            }
        }

        crate::AuthCommands::Status => {
            let (has_sesskey, sesskey, ws_token) = auth::check_session_status(&config);
            let active = has_sesskey || ws_token.is_some();

            let session_path = Path::new(&config.auth_state_path);
            let auth_dir = session_path.parent().unwrap_or(Path::new(".auth"));
            let cookies_path = auth_dir.join("cookies.json");
            let meta_path = auth_dir.join("session-meta.json");
            let session_exists = session_path.exists();
            let cookies_exists = cookies_path.exists();

            let stats_source = if cookies_exists {
                Some(cookies_path.as_path())
            } else if session_exists {
                Some(session_path)
            } else {
                None
            };

            let (size, modified) = match stats_source.and_then(|p| std::fs::metadata(p).ok()) {
                Some(md) => {
                    let size = Some(md.len());
                    let modified = md.modified().ok()
                        .map(chrono::DateTime::<chrono::Utc>::from)
                        .map(|dt| dt.to_rfc3339());
                    (size, modified)
                }
                None => (None, None),
            };

            let moodle_session_cookie = std::fs::read_to_string(&cookies_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<Vec<auth::Cookie>>(&raw).ok())
                .and_then(|cookies| cookies.into_iter().find(|c| c.name == "MoodleSession"));

            let moodle_session_expires = moodle_session_cookie
                .as_ref()
                .and_then(|c| c.expires)
                .and_then(|ts| if ts > 0.0 { Some(ts) } else { None })
                .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
                .map(|dt| dt.to_rfc3339());

            if active {
                log.success("Session active");
                if let Some(sk) = &sesskey {
                    log.info(&format!("  sesskey: {}", sk));
                }
                if let Some(wt) = &ws_token {
                    log.info(&format!("  WS token: {}...", &wt[..wt.len().min(20)]));
                }
            } else {
                log.warn("No active session found. Run 'openape login' to log in.");
            }

            let result = serde_json::json!({
                "action": "status",
                "status": if active { "success" } else { "error" },
                "session_path": config.auth_state_path,
                "cookies_path": cookies_path.to_string_lossy(),
                "meta_path": meta_path.to_string_lossy(),
                "exists": session_exists || cookies_exists,
                "modified": modified,
                "size": size,
                "moodle_session": {
                    "exists": moodle_session_cookie.is_some(),
                    "expires": moodle_session_expires,
                },
                "active": active,
                "sesskey": sesskey.as_deref().map(|s| &s[..s.len().min(20)]),
                "ws_token_prefix": ws_token.as_deref().map(|t| &t[..t.len().min(20)]),
                "hint": if active { None } else { Some("Run 'openape login' first") },
            });
            format_and_output(&[result], cli.output, None);
        }

        crate::AuthCommands::Logout => {
            auth::logout(&config);
            log.success("Session cleared.");
            let result = serde_json::json!({
                "action": "logout",
                "success": true,
            });
            format_and_output(&[result], cli.output, None);
        }
    }

    Ok(())
}
