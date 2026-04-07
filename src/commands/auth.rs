use anyhow::Result;
use crate::Cli;
use crate::config::load_config;
use crate::logger::Logger;
use crate::auth;
use crate::output::format_and_output;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CRATES_IO_API: &str = "https://crates.io/api/v1/crates/openape";

async fn check_for_update(log: &Logger) {
    let client = reqwest::Client::builder()
        .user_agent(format!("openape/{}", CURRENT_VERSION))
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let Ok(client) = client else { return };

    let Ok(resp) = client.get(CRATES_IO_API).send().await else { return };
    let Ok(json) = resp.json::<serde_json::Value>().await else { return };

    if let Some(latest) = json["crate"]["newest_version"].as_str() {
        if latest != CURRENT_VERSION {
            log.warn(&format!(
                "Update available: v{} → v{}  (run: cargo install openape)",
                CURRENT_VERSION, latest
            ));
        }
    }
}

pub async fn run(cmd: &crate::AuthCommands, cli: &Cli) -> Result<()> {
    let config = load_config(cli.config.as_ref().and_then(|p| p.parent()));
    let log = Logger::new(cli.verbose, cli.silent);

    match cmd {
        crate::AuthCommands::Login => {
            check_for_update(&log).await;
            log.info("Launching browser for login...");
            let (_browser, ws_token) = auth::launch_authenticated(&config, &log).await?;
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
            if active {
                log.success("Session active");
                if let Some(sk) = &sesskey {
                    log.info(&format!("  sesskey: {}", sk));
                }
                if let Some(wt) = &ws_token {
                    log.info(&format!("  WS token: {}...", &wt[..wt.len().min(20)]));
                }
            } else {
                log.warn("No active session found. Run 'openape auth login' to log in.");
            }
            let result = serde_json::json!({
                "action": "status",
                "active": active,
                "sesskey": sesskey.as_deref().map(|s| &s[..s.len().min(20)]),
                "ws_token_prefix": ws_token.as_deref().map(|t| &t[..t.len().min(20)]),
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
