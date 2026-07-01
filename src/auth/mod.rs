//! Authentication system: login, session management, token acquisition.

pub mod browser;
mod credentials;
mod microsoft;
mod token;

use browser::get_user_data_dir;
pub use browser::{
    close_browser, cookies_to_cookie_header, find_browser_paths, get_cookies, launch_browser,
    set_cookies, Cookie, LaunchedBrowser,
};
pub use credentials::StoredCredentials;
use token::{extract_token_from_custom_scheme, SessionMeta};

use std::fs;
use std::path::Path;

use crate::config::AppConfig;
use crate::logger::Logger;
use crate::moodle::types::SessionInfo;
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, DisableParams as FetchDisable, EnableParams as FetchEnable,
    EventRequestPaused, RequestPattern, RequestStage,
};
use chromiumoxide::Page;
use futures::StreamExt;

/// Launch browser, restore/create session, acquire WS token.
pub async fn launch_authenticated(
    config: &AppConfig,
    log: &Logger,
) -> anyhow::Result<(LaunchedBrowser, Option<String>)> {
    launch_authenticated_with(config, log, None).await
}

pub async fn launch_authenticated_auto(
    config: &AppConfig,
    student_id: &str,
    password: &str,
    log: &Logger,
) -> anyhow::Result<(LaunchedBrowser, Option<String>)> {
    for attempt in 0..3 {
        match launch_authenticated_with(config, log, Some((student_id, password))).await {
            Ok(session) => return Ok(session),
            Err(error)
                if attempt < 2
                    && error
                        .downcast_ref::<microsoft::SessionExchangeError>()
                        .is_some() =>
            {
                log.warn(&format!(
                    "Login session exchange failed, retrying with a fresh browser ({}/3).",
                    attempt + 2
                ));
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

async fn launch_authenticated_with(
    config: &AppConfig,
    log: &Logger,
    credentials: Option<(&str, &str)>,
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

    for exe_path in &browser_candidates {
        match launch_browser(exe_path, credentials.is_some(), None).await {
            Ok(l) => {
                launched_opt = Some(l);
                break;
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!("Launch failed on {}: {}", exe_path, e));
            }
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
        let ws_token = match finalize_session(&launched.page, &mut meta, config, log).await {
            Ok(token) => token,
            Err(error) => {
                close_browser(launched).await;
                return Err(error);
            }
        };
        log.success("Session restored successfully.");
        return Ok((launched, ws_token));
    }

    let login_result = if let Some((student_id, password)) = credentials {
        microsoft::perform_headless_login(
            &launched.page,
            &config.moodle_base_url,
            student_id,
            password,
            log,
        )
        .await
    } else {
        perform_login(&launched.page, &config.moodle_base_url, log).await
    };
    if let Err(error) = login_result {
        close_browser(launched).await;
        return Err(error);
    }
    let ws_token = match finalize_session(&launched.page, &mut meta, config, log).await {
        Ok(token) => token,
        Err(error) => {
            close_browser(launched).await;
            return Err(error);
        }
    };

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
        save_cookies(&config.auth_state_path, &cookies)?;
    }

    meta.save(&config.auth_state_path)?;
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

/// Intercept the Microsoft `authorize` navigation and append `prompt=login` before it is
/// ever sent. On shared/managed Windows machines Edge injects an OS-level token (WAM/PRT)
/// into the first authorize request and Microsoft silently signs in a cached account. A
/// reactive URL check can't win that race — by the time we observe a URL the redirect has
/// already bounced back. `prompt=login` forces the IdP to show the credential page even
/// when a valid SSO token is presented, and the IdP honors it regardless of device policy.
async fn force_account_prompt(page: &Page) -> tokio::task::JoinHandle<()> {
    let pattern = RequestPattern::builder()
        .url_pattern("*login.microsoftonline.com*authorize*")
        .request_stage(RequestStage::Request)
        .build();
    if page
        .execute(FetchEnable::builder().pattern(pattern).build())
        .await
        .is_err()
    {
        return tokio::spawn(async {});
    }
    let Ok(mut paused) = page.event_listener::<EventRequestPaused>().await else {
        return tokio::spawn(async {});
    };
    let page = page.clone();
    tokio::spawn(async move {
        while let Some(ev) = paused.next().await {
            let url = &ev.request.url;
            let cmd = if url.contains("prompt=") {
                None
            } else {
                let sep = if url.contains('?') { "&" } else { "?" };
                ContinueRequestParams::builder()
                    .request_id(ev.request_id.clone())
                    .url(format!("{url}{sep}prompt=login"))
                    .build()
                    .ok()
            };
            let cmd = cmd.unwrap_or_else(|| ContinueRequestParams::new(ev.request_id.clone()));
            let _ = page.execute(cmd).await;
        }
    })
}

/// Perform Microsoft OAuth login flow.
async fn perform_login(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<()> {
    log.info("Starting Microsoft OAuth login...");

    // Force a fresh account prompt so Edge can't silently reuse a cached OS account.
    let interceptor = force_account_prompt(page).await;

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
            interceptor.abort();
            anyhow::bail!("Login timed out waiting for redirect.");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let _ = page.execute(FetchDisable::default()).await;
    interceptor.abort();
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

/// Remove saved browser/API session files.
pub fn clear_saved_session(config: &AppConfig) {
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

    if let Some(auth_dir) = auth_state_path.parent() {
        let legacy_credentials = auth_dir.join("credentials.json");
        if legacy_credentials.exists() {
            let _ = std::fs::remove_file(legacy_credentials);
        }
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

/// Remove saved session files and automatic-login credentials.
pub fn logout(config: &AppConfig) -> anyhow::Result<()> {
    clear_saved_session(config);
    StoredCredentials::delete()
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

    match StoredCredentials::load() {
        Ok(Some(credentials)) => {
            log.info("Session expired. Attempting automatic login...");
            for attempt in 0..2 {
                for exe_path in &browser_candidates {
                    let Ok(launched) = launch_browser(exe_path, true, None).await else {
                        continue;
                    };
                    match microsoft::perform_headless_login(
                        &launched.page,
                        &config.moodle_base_url,
                        &credentials.id,
                        &credentials.password,
                        log,
                    )
                    .await
                    {
                        Ok(()) => {
                            let mut meta = SessionMeta::load(&config.auth_state_path);
                            if let Err(error) =
                                finalize_session(&launched.page, &mut meta, config, log).await
                            {
                                close_browser(launched).await;
                                return Err(error);
                            }
                            log.success("Automatic login succeeded.");
                            return Ok(launched);
                        }
                        Err(error) => {
                            log.warn(&format!("Automatic login failed: {error}"));
                            close_browser(launched).await;
                            if error
                                .downcast_ref::<microsoft::SessionExchangeError>()
                                .is_none()
                            {
                                return Err(error);
                            }
                        }
                    }
                }
                if attempt == 0 {
                    log.info("Retrying automatic login with a fresh browser...");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
        Ok(None) => {}
        Err(error) => log.warn(&format!("Could not read OS credential store: {error}")),
    }

    anyhow::bail!("Browser session expired. Run `openape login` to re-authenticate.")
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

fn save_cookies(auth_state_path: &str, cookies: &[Cookie]) -> anyhow::Result<()> {
    let path = get_cookies_path(auth_state_path);
    let json = serde_json::to_string_pretty(cookies)?;
    write_secret_file(&path, json.as_bytes())?;
    Ok(())
}

fn write_secret_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.set_len(0)?;
        file.write_all(contents)
    }

    #[cfg(not(unix))]
    fs::write(path, contents)
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

#[cfg(all(test, unix))]
mod tests {
    use super::write_secret_file;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn secret_writer_tightens_existing_permissions() {
        let root = std::env::temp_dir().join(format!(
            "openape-secret-writer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let auth_dir = root.join(".auth");
        let secret = auth_dir.join("secret.json");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&secret, b"old").unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_secret_file(&secret, b"new").unwrap();

        assert_eq!(std::fs::read(&secret).unwrap(), b"new");
        assert_eq!(
            std::fs::metadata(&auth_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&secret).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
