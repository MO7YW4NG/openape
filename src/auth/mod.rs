//! Authentication system: login, session management, token acquisition.

mod browser;
mod token;

pub use browser::{Cookie, cookies_to_cookie_header, get_cookies, LaunchedBrowser};
use browser::{close_browser, find_browser_path, launch_browser, get_user_data_dir, set_cookies};
use token::{extract_token_from_custom_scheme, SessionMeta};

use std::path::Path;

use crate::config::AppConfig;
use crate::logger::Logger;
use crate::moodle::types::SessionInfo;
use chromiumoxide::Page;
use futures::StreamExt;

/// Launch browser, restore/create session, acquire WS token.
pub async fn launch_authenticated(config: &AppConfig, log: &Logger) -> anyhow::Result<(LaunchedBrowser, Option<String>)> {
    let exe_path = find_browser_path()?;
    log.debug(&format!("Using browser: {}", exe_path));

    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    
    // Load saved session metadata
    let mut meta = SessionMeta::load(&config.auth_state_path);
    let mut ws_token = meta.get_ws_token();
    if ws_token.is_some() {
        log.info("Loaded saved Web Service Token.");
    }

    // Try launching with persistent user data directory
    let launched = launch_browser(&exe_path, config.headless, Some(&user_data_dir)).await?;

    // Check if session is still valid
    let session_valid = check_session_valid(&launched.page, &config.moodle_base_url).await;

    if session_valid {
        // Try to acquire WS token if we don't have one
        if ws_token.is_none() {
            match acquire_ws_token(&launched.page, &config.moodle_base_url, log).await {
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
        log.success("Session restored successfully.");
        return Ok((launched, ws_token));
    }

    // Session invalid - need to login
    // If headless, close and relaunch headed
    if config.headless {
        close_browser(launched).await;
        let launched = launch_browser(&exe_path, false, Some(&user_data_dir)).await?;
        perform_login(&launched.page, &config.moodle_base_url, log).await?;
        
        // Acquire WS token
        if ws_token.is_none() {
            match acquire_ws_token(&launched.page, &config.moodle_base_url, log).await {
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
        
        // Save cookies
        if let Ok(cookies) = get_cookies(&launched.page).await {
            save_cookies(&config.auth_state_path, &cookies);
        }
        
        return Ok((launched, ws_token));
    }

    // Already headed, just login
    perform_login(&launched.page, &config.moodle_base_url, log).await?;
    
    // Acquire WS token
    if ws_token.is_none() {
        match acquire_ws_token(&launched.page, &config.moodle_base_url, log).await {
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
    
    // Save cookies
    if let Ok(cookies) = get_cookies(&launched.page).await {
        save_cookies(&config.auth_state_path, &cookies);
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

/// Check if session is valid by navigating to dashboard.
async fn check_session_valid(page: &Page, base_url: &str) -> bool {
    let result = page.goto(&format!("{}/my/", base_url)).await;
    
    if result.is_err() {
        return false;
    }
    
    // Wait a bit for redirect
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    
    if let Ok(Some(url)) = page.url().await {
        // If redirected to login, session is invalid
        !url.contains("login") && !url.contains("microsoftonline")
    } else {
        false
    }
}

/// Perform Microsoft OAuth login flow.
async fn perform_login(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<()> {
    log.info("Starting Microsoft OAuth login...");

    page.goto(&format!("{}/login/index.php", base_url))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to navigate to login: {}", e))?;

    log.info("Microsoft login page detected. Please complete login in the browser.");
    log.info("Waiting for redirect back to Moodle...");

    // Wait up to 5 minutes for user to complete login
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
    loop {
        if let Ok(Some(url)) = page.url().await {
            if url.contains(&base_url.replace("https://", ""))
                && !url.contains("login")
                && !url.contains("microsoftonline")
            {
                break;
            }
        }
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Login timed out waiting for redirect.");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    log.success("Login completed successfully.");
    Ok(())
}

/// Acquire Moodle Web Service Token via mobile app launch endpoint.
async fn acquire_ws_token(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<String> {
    log.info("Acquiring Moodle Web Service Token...");

    let passport = uuid::Uuid::new_v4().to_string();
    let launch_url = format!(
        "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={}",
        base_url, passport
    );

    log.debug(&format!("Token acquisition URL: {}", launch_url));

    // Use network interception to catch redirect
    use chromiumoxide::cdp::browser_protocol::network::EventResponseReceived;
    
    let mut response_events = page.event_listener::<EventResponseReceived>().await
        .map_err(|e| anyhow::anyhow!("Failed to create event listener: {}", e))?;

    // Navigate (will get redirected to moodlemobile:// which browser can't handle)
    let _ = page.goto(&launch_url).await;

    // Listen for the redirect response
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
    
    while let Ok(result) = tokio::time::timeout(
        deadline.saturating_duration_since(tokio::time::Instant::now()),
        response_events.next()
    ).await {
        if let Some(event) = result {
            // Check response URL for moodlemobile:// redirect
            let response_url = &event.response.url;
            if response_url.starts_with("moodlemobile://") {
                if let Some(token) = extract_token_from_custom_scheme(response_url) {
                    log.success("Web Service Token acquired successfully.");
                    return Ok(token);
                }
            }
            
            // Also check status code for redirect (302/307)
            if event.response.status == 302 || event.response.status == 307 {
                // Try to get Location header - chromiumoxide headers is a HashMap-like structure
                // We need to iterate through headers to find Location
                // Note: This is a simplified approach - we rely on response_url being set
                log.debug(&format!("Got redirect with status {}", event.response.status));
            }
        }
    }

    anyhow::bail!("Token acquisition timed out - no moodlemobile:// redirect received.")
}

/// Check session status without launching a browser.
pub fn check_session_status(config: &AppConfig) -> (bool, Option<String>, Option<String>) {
    let meta = SessionMeta::load(&config.auth_state_path);
    let ws_token = meta.get_ws_token();
    let sesskey = meta.get_sesskey();
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    let has_data_dir = user_data_dir.exists();

    (has_data_dir, sesskey, ws_token)
}

/// Remove saved session files.
pub fn logout(config: &AppConfig) {
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    if user_data_dir.exists() {
        let _ = std::fs::remove_dir_all(&user_data_dir);
    }
    let meta_path_str = SessionMeta::meta_path(&config.auth_state_path);
    let meta_path = Path::new(&meta_path_str);
    if meta_path.exists() {
        let _ = std::fs::remove_file(meta_path);
    }
    // Also remove cookies file
    let cookies_path = get_cookies_path(&config.auth_state_path);
    if cookies_path.exists() {
        let _ = std::fs::remove_file(&cookies_path);
    }
}

/// Launch a headless browser session using saved session data.
pub async fn launch_persistent_session(config: &AppConfig, log: &Logger) -> anyhow::Result<LaunchedBrowser> {
    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    let cookies_path = get_cookies_path(&config.auth_state_path);

    let has_persistent = user_data_dir.exists();
    let has_cookies = cookies_path.exists();

    if !has_persistent && !has_cookies {
        anyhow::bail!("No browser session found. Run `openape login` first.");
    }

    let exe_path = find_browser_path()?;
    log.debug(&format!("Using browser: {}", exe_path));

    let launched = launch_browser(
        &exe_path,
        true,
        if has_persistent { Some(&user_data_dir) } else { None }
    ).await?;

    // If we have saved cookies but no persistent dir, restore them
    if !has_persistent && has_cookies {
        if let Ok(cookies) = load_cookies(&config.auth_state_path) {
            let _ = set_cookies(&launched.page, &cookies).await;
        }
    }

    // Validate session
    let check_url = format!("{}/my/", config.moodle_base_url);
    let result = launched.page.goto(&check_url).await;

    if result.is_err() {
        close_browser(launched).await;
        anyhow::bail!("Failed to navigate. Run `openape login` first.");
    }

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    if let Ok(Some(url)) = launched.page.url().await {
        if url.contains("login") || url.contains("microsoftonline") {
            close_browser(launched).await;
            anyhow::bail!("Browser session expired. Run `openape login` to re-authenticate.");
        }
    }

    log.info("Browser session restored.");
    Ok(launched)
}

/// Close a browser session.
pub async fn close_persistent_session(launched: LaunchedBrowser) {
    close_browser(launched).await;
}

// Helper functions for cookie persistence

fn get_cookies_path(auth_state_path: &str) -> std::path::PathBuf {
    let path = Path::new(auth_state_path);
    let dir = path.parent().unwrap_or(Path::new(".auth"));
    dir.join("cookies.json")
}

fn save_cookies(auth_state_path: &str, cookies: &[Cookie]) {
    let path = get_cookies_path(auth_state_path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cookies) {
        let _ = std::fs::write(&path, json);
    }
}

fn load_cookies(auth_state_path: &str) -> anyhow::Result<Vec<Cookie>> {
    let path = get_cookies_path(auth_state_path);
    let content = std::fs::read_to_string(&path)?;
    let cookies: Vec<Cookie> = serde_json::from_str(&content)?;
    Ok(cookies)
}
