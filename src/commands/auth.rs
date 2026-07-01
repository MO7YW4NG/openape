use crate::auth;
use crate::config::load_config_for_cli;
use crate::logger::Logger;
use crate::output::format_and_output;
use crate::Cli;
use anyhow::Result;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const NPM_PACKAGE_NAME: &str = "@mo7yw4ng/openape";
const NPM_REGISTRY_API: &str = "https://registry.npmjs.org/@mo7yw4ng%2fopenape";

fn version_tuple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?
        .split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    matches!(
        (version_tuple(latest), version_tuple(current)),
        (Some(latest), Some(current)) if latest > current
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginMethod {
    Browser,
    Automatic,
}

fn parse_login_method(input: &str) -> Option<LoginMethod> {
    match input.trim() {
        "1" => Some(LoginMethod::Browser),
        "2" => Some(LoginMethod::Automatic),
        _ => None,
    }
}

fn login_progress_frame(tick: usize) -> String {
    format!("\rSigning in{:<3}", ".".repeat(tick % 3 + 1))
}

fn prompt_login_method() -> Result<LoginMethod> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("Login requires an interactive terminal.");
    }

    eprintln!("Login method:");
    eprintln!("  1) Browser");
    eprintln!("  2) Automatic (OS credential store)");
    eprint!("Select [1-2]: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    parse_login_method(&input).ok_or_else(|| anyhow::anyhow!("Login method must be 1 or 2."))
}

fn prompt_new_credentials() -> Result<auth::StoredCredentials> {
    eprint!("Student ID: ");
    io::stderr().flush()?;

    let mut id = String::new();
    io::stdin().read_line(&mut id)?;
    if id.trim().is_empty() {
        anyhow::bail!("Student ID must not be empty.");
    }

    let password = rpassword::prompt_password_with_config(
        "Password: ",
        rpassword::ConfigBuilder::new()
            .password_feedback_mask('*')
            .build(),
    )?;
    auth::StoredCredentials::new(id, password)
}

async fn check_for_update(log: &Logger) {
    let client = reqwest::Client::builder()
        .user_agent(format!("openape/{}", CURRENT_VERSION))
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let Ok(client) = client else { return };

    let Ok(resp) = client.get(NPM_REGISTRY_API).send().await else {
        return;
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return;
    };

    if json.get("error").is_some() {
        log.debug("Update check skipped: package not found on npm registry.");
        return;
    }

    if let Some(latest) = json["dist-tags"]["latest"].as_str() {
        if is_newer_version(latest, CURRENT_VERSION) {
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
            let saved_credentials = match auth::StoredCredentials::load() {
                Ok(credentials) => credentials,
                Err(error) => {
                    log.warn(&format!("Could not read OS credential store: {error}"));
                    None
                }
            };
            let method = if saved_credentials.is_some() {
                LoginMethod::Automatic
            } else {
                prompt_login_method()?
            };
            check_for_update(&log).await;

            let (launched, ws_token) = match method {
                LoginMethod::Browser => {
                    if let Err(error) = auth::StoredCredentials::delete() {
                        log.warn(&format!(
                            "Could not clear saved automatic-login credentials: {error}"
                        ));
                    }
                    auth::clear_saved_session(&config);
                    log.info("Launching browser for login...");
                    auth::launch_authenticated(&config, &log).await?
                }
                LoginMethod::Automatic => {
                    let credentials = match saved_credentials {
                        Some(credentials) => credentials,
                        None => prompt_new_credentials()?,
                    };
                    log.info("Starting automatic login...");
                    let progress = (!cli.verbose && !cli.silent).then(|| {
                        tokio::spawn(async {
                            let mut tick = 0;
                            loop {
                                eprint!("{}", login_progress_frame(tick));
                                let _ = io::stderr().flush();
                                tick += 1;
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            }
                        })
                    });
                    let login = auth::launch_authenticated_auto(
                        &config,
                        &credentials.id,
                        &credentials.password,
                        &log,
                    )
                    .await;
                    if let Some(progress) = progress {
                        progress.abort();
                        let _ = progress.await;
                        eprint!("\r              \r");
                        io::stderr().flush()?;
                    }
                    let (launched, ws_token) = login?;
                    if let Err(error) = credentials.save() {
                        auth::close_persistent_session(launched).await;
                        return Err(error);
                    }
                    (launched, ws_token)
                }
            };
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
                    log.warn(
                        "Logged in but could not acquire WS token. Some commands may not work.",
                    );
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
            let (has_session, ws_token) = auth::check_session_status(&config);
            let active = has_session || ws_token.is_some();

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
                    let modified = md
                        .modified()
                        .ok()
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
                "ws_token_prefix": ws_token.as_deref().map(|t| &t[..t.len().min(20)]),
                "hint": if active { None } else { Some("Run 'openape login' first") },
            });
            format_and_output(&[result], cli.output, None);
        }

        crate::AuthCommands::Logout => {
            auth::logout(&config)?;
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

#[cfg(test)]
mod tests {
    use super::{parse_login_method, LoginMethod};

    #[test]
    fn parses_login_method_selection() {
        assert_eq!(parse_login_method(" 1\n"), Some(LoginMethod::Browser));
        assert_eq!(parse_login_method("2"), Some(LoginMethod::Automatic));
        assert_eq!(parse_login_method(""), None);
        assert_eq!(parse_login_method("automatic"), None);
    }

    #[test]
    fn cycles_login_progress_dots() {
        assert_eq!(super::login_progress_frame(0), "\rSigning in.  ");
        assert_eq!(super::login_progress_frame(2), "\rSigning in...");
        assert_eq!(super::login_progress_frame(3), "\rSigning in.  ");
    }

    #[test]
    fn only_reports_newer_versions() {
        assert!(super::is_newer_version("2.1.6", "2.1.5"));
        assert!(!super::is_newer_version("2.1.4", "2.1.5"));
        assert!(!super::is_newer_version("2.1.5", "2.1.5"));
    }
}
