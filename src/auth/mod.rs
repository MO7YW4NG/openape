//! Authentication system: login, session management, token acquisition.

mod browser;
mod token;

use browser::{close_browser, find_browser_path, launch_playwright};
use token::{extract_token_from_custom_scheme, SessionMeta};

use std::path::Path;

use crate::config::AppConfig;
use crate::logger::Logger;
use playwright_rs::Playwright;
use crate::moodle::types::SessionInfo;

/// Launch browser, restore/create session, acquire WS token.
pub async fn launch_authenticated(config: &AppConfig, log: &Logger) -> anyhow::Result<(browser::LaunchedBrowser, Option<String>)> {
    let exe_path = find_browser_path()?;
    log.debug(&format!("Using browser: {}", exe_path));

    // Wait to ensure previous browser process has terminated
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let launched = launch_playwright(&exe_path, config.headless).await?;

    // Try loading saved WS token
    let mut meta = SessionMeta::load(&config.auth_state_path);
    let mut ws_token = meta.get_ws_token();
    if ws_token.is_some() {
        log.info("Loaded saved Web Service Token.");
    }

    // Try restoring saved session via persistent context
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    let context_restored = try_restore_persistent_context(
        &launched.playwright,
        &user_data_dir,
        &config.moodle_base_url,
    )
    .await;

    if context_restored {
        // Try to acquire WS token if we don't have one
        if ws_token.is_none() {
            // Need a page from the persistent context
            let pages = launched.context.pages();
            if !pages.is_empty() {
                match acquire_ws_token_via_route(&pages[0], &config.moodle_base_url, log).await {
                    Ok(token) => {
                        meta.set_ws_token(&token);
                        meta.save(&config.auth_state_path);
                        ws_token = Some(token);
                    }
                    Err(e) => {
                        log.warn(&format!("Failed to acquire WS Token: {}", e));
                    }
                }
            }
        }
        log.success("Session restored successfully.");
        return Ok((launched, ws_token));
    }

    // No saved session - close headless attempt and launch headed for login
    close_browser(launched).await;
    let launched = launch_playwright(&exe_path, false).await?;

    perform_login(&launched.page, &config.moodle_base_url, log).await?;

    // Save session via persistent context (cookies are preserved automatically)
    log.debug(&format!("Session saved to {}", user_data_dir));

    // Acquire WS token
    if ws_token.is_none() {
        let pages = launched.context.pages();
        if !pages.is_empty() {
            match acquire_ws_token_via_route(&pages[0], &config.moodle_base_url, log).await {
                Ok(token) => {
                    meta.set_ws_token(&token);
                    meta.save(&config.auth_state_path);
                    ws_token = Some(token);
                }
                Err(e) => {
                    log.warn(&format!("Failed to acquire WS Token: {}", e));
                }
            }
        }
    }

    Ok((launched, ws_token))
}

/// Create API-only context (no browser) using saved WS token.
pub fn create_api_context(config: &AppConfig, log: &Logger) -> anyhow::Result<SessionInfo> {
    let meta = SessionMeta::load(&config.auth_state_path);
    let ws_token = meta.get_ws_token().ok_or_else(|| {
        anyhow::anyhow!("No WS token found. Run `openape login` first.")
    })?;

    log.debug("Using cached Web Service Token.");
    Ok(SessionInfo {
        moodle_base_url: config.moodle_base_url.clone(),
        ws_token: Some(ws_token),
    })
}

/// Perform Microsoft OAuth login flow.
async fn perform_login(
    page: &playwright_rs::Page,
    base_url: &str,
    log: &Logger,
) -> anyhow::Result<()> {
    log.info("Starting Microsoft OAuth login...");

    page.goto(
        &format!("{}/login/index.php", base_url),
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to navigate to login: {}", e))?;

    log.info("Microsoft login page detected. Please complete login in the browser.");
    log.info("Waiting for redirect back to Moodle...");

    // Wait up to 5 minutes for user to complete login
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
    loop {
        let url = page.url();
        if url.contains(&base_url.replace("https://", ""))
            && !url.contains("login")
            && !url.contains("microsoftonline")
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Login timed out waiting for redirect.");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    log.success("Login completed successfully.");
    Ok(())
}

/// Try restoring a session via persistent context (user data dir).
/// Returns true if session was successfully restored.
async fn try_restore_persistent_context(
    playwright: &Playwright,
    user_data_dir: &str,
    base_url: &str,
) -> bool {
    let context: playwright_rs::BrowserContext = match playwright
        .chromium()
        .launch_persistent_context(user_data_dir)
        .await
    {
        Ok(ctx) => ctx,
        Err(_) => return false,
    };

    let pages = context.pages();

    // Try navigating to check session validity
    if !pages.is_empty() {
        let result = pages[0]
            .goto(&format!("{}/my/", base_url), None)
            .await;

        if let Ok(Some(_resp)) = result {
            let url = pages[0].url();
            if url.contains("login") || url.contains("microsoftonline") {
                let _ = context.close().await;
                return false;
            }
        }
    }

    // Session is valid, but we need to keep the context alive
    // Unfortunately we can't easily transfer ownership back, so return false
    // and re-launch with the same user_data_dir
    let _ = context.close().await;
    true
}

/// Acquire Moodle Web Service Token via mobile app launch endpoint.
///
/// Process:
/// 1. Visit admin/tool/mobile/launch.php?service=moodle_mobile_app&passport=UUID
/// 2. Server responds with 302 Location: moodlemobile://token=BASE64_DATA
/// 3. Listen to response events to catch the Location header and extract token
pub async fn acquire_ws_token_via_route(
    page: &playwright_rs::Page,
    base_url: &str,
    log: &Logger,
) -> anyhow::Result<String> {
    log.info("Acquiring Moodle Web Service Token...");

    let passport = uuid::Uuid::new_v4().to_string();
    let launch_url = format!(
        "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={}",
        base_url, passport
    );

    log.debug(&format!("Token acquisition URL: {}", launch_url));

    // Set up response listener before navigating
    let captured_token = std::sync::Arc::new(tokio::sync::Mutex::new(None::<String>));
    let captured = captured_token.clone();

    page.on_response(move |response| {
        let captured = captured.clone();
        async move {
            // Check Location header for moodlemobile:// redirect
            if let Ok(headers) = response.raw_headers().await {
                for h in &headers {
                    if h.name.to_lowercase() == "location"
                        && h.value.starts_with("moodlemobile://")
                    {
                        if let Some(token) = extract_token_from_custom_scheme(&h.value) {
                            *captured.lock().await = Some(token);
                        }
                        break;
                    }
                }
            }
            Ok(())
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("Failed to register response listener: {}", e))?;

    // Navigate (browser can't open moodlemobile://, navigation will fail - that's expected)
    let _ = page.goto(&launch_url, None).await;

    // Wait for captured token
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
    loop {
        let guard = captured_token.lock().await;
        if let Some(ref t) = *guard {
            log.success("Web Service Token acquired successfully.");
            return Ok(t.clone());
        }
        drop(guard);
        if tokio::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    anyhow::bail!("Token acquisition timed out - no moodlemobile:// redirect received.")
}

/// Check session status without launching a browser.
pub fn check_session_status(config: &AppConfig) -> (bool, Option<String>, Option<String>) {
    let meta = SessionMeta::load(&config.auth_state_path);
    let ws_token = meta.get_ws_token();
    let sesskey = meta.get_sesskey();
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    let has_data_dir = Path::new(&user_data_dir).exists();

    (has_data_dir, sesskey, ws_token)
}

/// Remove saved session files.
pub fn logout(config: &AppConfig) {
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    let dir = Path::new(&user_data_dir);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
    let meta_path_str = SessionMeta::meta_path(&config.auth_state_path);
    let meta_path = Path::new(&meta_path_str);
    if meta_path.exists() {
        let _ = std::fs::remove_file(meta_path);
    }
}

/// Get the user data directory path from the auth state path.
fn get_user_data_dir(auth_state_path: &str) -> String {
    let path = Path::new(auth_state_path);
    let dir = path.parent().unwrap_or(Path::new(".auth"));
    dir.join("browser-data").to_string_lossy().to_string()
}
