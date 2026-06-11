//! Authentication system: login, session management, token acquisition.

pub mod browser;
pub mod credentials;
mod microsoft;
mod token;

use browser::get_user_data_dir;
pub use browser::{
    close_browser, cookies_to_cookie_header, find_browser_paths, get_cookies, launch_browser,
    set_cookies, Cookie, LaunchedBrowser,
};
pub use credentials::StoredCredentials;
use microsoft::perform_headless_login;
use token::{extract_token_from_custom_scheme, SessionMeta};

use std::path::Path;

use crate::config::AppConfig;
use crate::logger::Logger;
use crate::moodle::types::SessionInfo;
use chromiumoxide::Page;

/// Launch browser, restore/create session, acquire WS token.
pub async fn launch_authenticated(
    config: &AppConfig,
    log: &Logger,
) -> anyhow::Result<(LaunchedBrowser, Option<String>)> {
    let browser_candidates = find_browser_paths();
    if browser_candidates.is_empty() {
        anyhow::bail!("No browser found (Edge/Chrome/Brave). Please install one.");
    }

    let cookies_path = get_cookies_path(&config.auth_state_path);
    let has_cookies = cookies_path.exists();

    // Load saved session metadata
    let mut meta = SessionMeta::load(&config.auth_state_path);
    // Always use clean profile + cookies (simplest and most reliable).
    log.info(&format!(
        "Found {} browser candidate(s)",
        browser_candidates.len()
    ));
    let mut launched_opt: Option<LaunchedBrowser> = None;
    let mut last_err: Option<anyhow::Error> = None;

    let headless_modes = if config.headless {
        vec![true]
    } else {
        vec![false]
    };

    'outer: for &use_headless in &headless_modes {
        for exe_path in &browser_candidates {
            match launch_browser(exe_path, use_headless, None).await {
                Ok(l) => {
                    launched_opt = Some(l);
                    break;
                }
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("Launch failed on {}: {}", exe_path, e));
                }
            }
        }
        if launched_opt.is_some() {
            break 'outer;
        }
    }
    let launched = match launched_opt {
        Some(l) => l,
        None => {
            return Err(last_err
                .unwrap_or_else(|| anyhow::anyhow!("Failed to launch any browser candidate")))
        }
    };

    // Restore cookies to clean profile (navigate first, then set cookies, then reload)
    if has_cookies {
        if let Ok(cookies) = load_cookies(&config.auth_state_path) {
            let _ = launched.page.goto(&config.moodle_base_url).await;
            let _ = launched.page.wait_for_navigation().await;
            let _ = set_cookies(&launched.page, &cookies).await;
            // Give the browser time to flush cookies to the store before reloading
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let _ = launched.page.reload().await;
            let _ = launched.page.wait_for_navigation().await;
        }
    }

    // Check if session is still valid
    let session_valid = check_session_valid(&launched.page, &config.moodle_base_url).await;

    if session_valid {
        let ws_token = finalize_session(&launched.page, &mut meta, config, log).await?;
        log.success("Session restored successfully.");
        return Ok((launched, ws_token));
    }

    // Session invalid - need to login
    // Try headless auto-login if credentials are stored and headless mode was requested.
    let creds = StoredCredentials::load(&config.auth_state_path);
    if let Some(ref c) = creds {
        if config.headless {
            log.info(&format!(
                "Stored credentials found. Attempting headless login for {}...",
                c.email()
            ));
            match perform_headless_login(&launched.page, &config.moodle_base_url, c, log).await {
                Ok(()) => {
                    log.success("Headless login succeeded.");
                    let ws_token = finalize_session(&launched.page, &mut meta, config, log).await?;
                    return Ok((launched, ws_token));
                }
                Err(e) => {
                    close_browser(launched).await;
                    anyhow::bail!("Headless login failed: {}", e);
                }
            }
        } else {
            log.info(
                "Stored credentials found, but headed mode was requested. Using interactive login.",
            );
        }
    }

    // No stored credentials - interactive login fallback
    // If headless, close and relaunch headed
    if config.headless {
        close_browser(launched).await;
        let browser_candidates = find_browser_paths();
        if browser_candidates.is_empty() {
            anyhow::bail!("No browser found (Edge/Chrome/Brave). Please install one.");
        }
        let mut relaunched: Option<LaunchedBrowser> = None;
        for exe_path in &browser_candidates {
            if let Ok(l) = launch_browser(exe_path, false, None).await {
                relaunched = Some(l);
                break;
            }
        }
        let launched = match relaunched {
            Some(l) => l,
            None => anyhow::bail!("Failed to relaunch browser in headed mode."),
        };
        perform_login(&launched.page, &config.moodle_base_url, log).await?;
        let ws_token = finalize_session(&launched.page, &mut meta, config, log).await?;

        return Ok((launched, ws_token));
    }

    // Already headed, just login
    perform_login(&launched.page, &config.moodle_base_url, log).await?;
    let ws_token = finalize_session(&launched.page, &mut meta, config, log).await?;

    Ok((launched, ws_token))
}

/// Create API-only context (no browser) using saved WS token.
pub fn create_api_context(config: &AppConfig, log: &Logger) -> anyhow::Result<SessionInfo> {
    let meta = SessionMeta::load(&config.auth_state_path);
    let ws_token = meta
        .get_ws_token()
        .ok_or_else(|| anyhow::anyhow!("No WS token found. Run `openape login` first."))?;

    log.debug("Using cached Web Service Token.");
    Ok(SessionInfo {
        moodle_base_url: config.moodle_base_url.clone(),
        ws_token: Some(ws_token),
        user_agent: meta.user_agent.clone(),
        user_id: meta.user_id.unwrap_or(0),
    })
}

/// Finalize a login session: capture user-agent, acquire WS token, save user ID and cookies.
async fn finalize_session(
    page: &Page,
    meta: &mut SessionMeta,
    config: &AppConfig,
    log: &Logger,
) -> anyhow::Result<Option<String>> {
    // Capture and save browser user-agent
    if let Some(ua) = get_browser_user_agent(page).await {
        meta.set_user_agent(&ua);
    }

    let ws_token = match acquire_ws_token(page, &config.moodle_base_url, log).await {
        Ok(token) => {
            meta.set_ws_token(&token);
            save_user_id(meta, &token, &config.moodle_base_url, log).await;
            Some(token)
        }
        Err(e) => {
            meta.clear_api_auth();
            log.warn(&format!("Failed to acquire WS Token: {}", e));
            None
        }
    };

    // Save cookies
    if let Ok(cookies) = get_cookies(page).await {
        save_cookies(&config.auth_state_path, &cookies);
    }

    meta.save(&config.auth_state_path);
    Ok(ws_token)
}

/// Check if session is valid by navigating to dashboard.
async fn check_session_valid(page: &Page, base_url: &str) -> bool {
    let result = page.goto(&format!("{}/my/", base_url)).await;

    if result.is_err() {
        return false;
    }

    // Wait for navigation/redirect to settle
    let _ = page.wait_for_navigation().await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

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

/// Get the browser's User-Agent via CDP.
async fn get_browser_user_agent(page: &Page) -> Option<String> {
    match page.evaluate("navigator.userAgent").await {
        Ok(val) => val.value().and_then(|v| v.as_str().map(String::from)),
        Err(_) => None,
    }
}

/// Acquire Moodle Web Service Token via mobile app launch endpoint.
async fn acquire_ws_token(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<String> {
    log.info("Acquiring Moodle Web Service Token...");

    let passport = uuid::Uuid::new_v4().to_string();
    let launch_url = format!(
        "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={}",
        base_url, passport
    );

    // More reliable than CDP event interception:
    // use current browser cookies and perform a direct HTTP request without following redirects.
    let cookies = get_cookies(page).await?;
    let cookie_header = cookies_to_cookie_header(&cookies, &launch_url);
    if cookie_header.is_empty() {
        anyhow::bail!("No cookies available for token acquisition.");
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

    let resp = client
        .get(&launch_url)
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Token request failed: {}", e))?;

    if let Some(loc) = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
    {
        if loc.starts_with("moodlemobile://") {
            if let Some(token) = extract_token_from_custom_scheme(loc) {
                log.success("Web Service Token acquired successfully.");
                return Ok(token);
            }
        }
        anyhow::bail!("Unexpected redirect location: {}", loc);
    }

    anyhow::bail!(
        "Token acquisition failed: no Location header received (status {}).",
        resp.status()
    )
}

/// Fetch and save user ID from Moodle site info API.
async fn save_user_id(meta: &mut SessionMeta, ws_token: &str, base_url: &str, _log: &Logger) {
    let url = format!(
        "{}/webservice/rest/server.php?wstoken={}&wsfunction=core_webservice_get_site_info&moodlewsrestformat=json",
        base_url, ws_token
    );
    if let Ok(resp) = reqwest::get(&url).await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(id) = json.get("userid").and_then(|v| v.as_u64()) {
                meta.set_user_id(id);
            }
        }
    }
}

/// Remove saved browser/API session files. Optionally keep stored credentials.
pub fn clear_saved_session(config: &AppConfig, clear_credentials: bool) {
    let auth_state_path = Path::new(&config.auth_state_path);
    if auth_state_path.exists() {
        let _ = std::fs::remove_file(auth_state_path);
    }

    let cookies_path = get_cookies_path(&config.auth_state_path);
    if cookies_path.exists() {
        let _ = std::fs::remove_file(&cookies_path);
    }

    let meta_path_str = SessionMeta::meta_path(&config.auth_state_path);
    let meta_path = Path::new(&meta_path_str);
    if meta_path.exists() {
        let _ = std::fs::remove_file(meta_path);
    }

    if clear_credentials {
        StoredCredentials::delete(&config.auth_state_path);
    }

    let user_data_dir = get_user_data_dir(&config.auth_state_path);
    if user_data_dir.exists() {
        let _ = std::fs::remove_dir_all(&user_data_dir);
    }
}

/// Check session status without launching a browser.
pub fn check_session_status(config: &AppConfig) -> (bool, Option<String>) {
    let meta = SessionMeta::load(&config.auth_state_path);
    let ws_token = meta.get_ws_token();
    let cookies_path = get_cookies_path(&config.auth_state_path);
    let has_session = cookies_path.exists();

    (has_session, ws_token)
}

/// Remove saved session files.
pub fn logout(config: &AppConfig) {
    clear_saved_session(config, true);
}

/// Launch a browser session using saved cookies (clean profile).
pub async fn launch_persistent_session(
    config: &AppConfig,
    log: &Logger,
    headless_only: bool,
) -> anyhow::Result<LaunchedBrowser> {
    let cookies_path = get_cookies_path(&config.auth_state_path);

    if !cookies_path.exists() {
        anyhow::bail!("No saved session found. Run `openape login` first.");
    }

    let browser_candidates = find_browser_paths();
    if browser_candidates.is_empty() {
        anyhow::bail!("No browser found (Edge/Chrome/Brave). Please install one.");
    }

    let headless_modes = if headless_only {
        vec![true]
    } else {
        vec![true, false]
    };

    for &use_headless in &headless_modes {
        let mode_label = if use_headless { "headless" } else { "headed" };

        let mut launched_opt: Option<LaunchedBrowser> = None;
        for exe_path in &browser_candidates {
            if let Ok(l) = launch_browser(exe_path, use_headless, None).await {
                launched_opt = Some(l);
                break;
            }
        }
        let launched = match launched_opt {
            Some(l) => l,
            None => continue,
        };

        // Restore cookies - must navigate to domain first, then set cookies, then reload
        if let Ok(cookies) = load_cookies(&config.auth_state_path) {
            let _ = launched.page.goto(&config.moodle_base_url).await;
            let _ = launched.page.wait_for_navigation().await;
            if let Err(e) = set_cookies(&launched.page, &cookies).await {
                log.warn(&format!(
                    "{} mode: failed to set cookies: {}",
                    mode_label, e
                ));
                close_browser(launched).await;
                continue;
            }
            // Give the browser time to flush cookies to the store before reloading
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let _ = launched.page.reload().await;
            let _ = launched.page.wait_for_navigation().await;
        }

        // Validate session
        let check_url = format!("{}/my/", config.moodle_base_url);
        if launched.page.goto(&check_url).await.is_err() {
            close_browser(launched).await;
            log.warn(&format!(
                "{} mode: navigation failed, trying next mode...",
                mode_label
            ));
            continue;
        }

        let _ = launched.page.wait_for_navigation().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let valid = if let Ok(Some(url)) = launched.page.url().await {
            !url.contains("login") && !url.contains("microsoftonline")
        } else {
            false
        };

        if !valid {
            close_browser(launched).await;
            log.warn(&format!(
                "{} mode: session invalid, trying next mode...",
                mode_label
            ));
            continue;
        }

        log.info("Browser session restored.");
        return Ok(launched);
    }

    // All session-restore attempts failed — try headless re-login if credentials exist
    let creds = StoredCredentials::load(&config.auth_state_path);
    if let Some(ref c) = creds {
        log.info(&format!(
            "Session expired. Attempting headless re-login for {}...",
            c.email()
        ));
        for exe_path in &browser_candidates {
            if let Ok(launched) = launch_browser(exe_path, true, None).await {
                match perform_headless_login(&launched.page, &config.moodle_base_url, c, log).await
                {
                    Ok(()) => {
                        log.success("Headless re-login succeeded.");
                        if let Ok(cookies) = get_cookies(&launched.page).await {
                            save_cookies(&config.auth_state_path, &cookies);
                        }
                        log.info("Browser session restored via re-login.");
                        return Ok(launched);
                    }
                    Err(e) => {
                        log.warn(&format!(
                            "Headless re-login failed with {}: {}",
                            exe_path, e
                        ));
                        close_browser(launched).await;
                    }
                }
            }
        }
        log.warn("Headless re-login failed with all browser candidates.");
    }

    anyhow::bail!("Browser session expired and headless re-login failed. Run `openape login` to re-authenticate.")
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

pub fn load_cookies(auth_state_path: &str) -> anyhow::Result<Vec<Cookie>> {
    let path = get_cookies_path(auth_state_path);
    let content = std::fs::read_to_string(&path)?;
    let cookies: Vec<Cookie> = serde_json::from_str(&content)?;
    Ok(cookies)
}

/// Load saved session cookies as a `Cookie: ...` header value for the given URL.
pub fn load_cookie_header(auth_state_path: &str, target_url: &str) -> Option<String> {
    let cookies = load_cookies(auth_state_path).ok()?;
    if cookies.is_empty() {
        return None;
    }
    let header = cookies_to_cookie_header(&cookies, target_url);
    if header.is_empty() {
        None
    } else {
        Some(header)
    }
}
