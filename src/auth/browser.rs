//! Browser lifecycle management using chromiumoxide (pure Rust CDP).

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::Page;
use futures::StreamExt;
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

/// Result of launching an authenticated session.
pub struct LaunchedBrowser {
    pub browser: Browser,
    pub page: Page,
    /// Background task handling browser events - must be kept alive
    pub handler: JoinHandle<()>,
    /// Temp directory to clean up on close (if any)
    pub temp_dir: Option<PathBuf>,
}

/// Find Chromium-based browser executables on the system (in priority order).
/// Priority: 64-bit Edge > Chrome > Brave > 32-bit Edge (last resort)
pub fn find_browser_paths() -> Vec<String> {
    let mut found = Vec::new();
    if cfg!(target_os = "windows") {
        let program_files = std::env::var("PROGRAMFILES").unwrap_or_default();
        let program_files_x86 = std::env::var("PROGRAMFILES(X86)").unwrap_or_default();
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();

        // Priority order: 64-bit browsers first, then 32-bit as fallback
        let browsers = [
            // 64-bit Edge (highest priority)
            (program_files.clone(), r"Microsoft\Edge\Application\msedge.exe"),
            (local_app_data.clone(), r"Microsoft\Edge\Application\msedge.exe"),
            // Chrome
            (program_files.clone(), r"Google\Chrome\Application\chrome.exe"),
            (program_files_x86.clone(), r"Google\Chrome\Application\chrome.exe"),
            // Brave
            (program_files.clone(), r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            // 32-bit Edge (last resort - often has CDP issues)
            (program_files_x86.clone(), r"Microsoft\Edge\Application\msedge.exe"),
        ];

        for (root, suffix) in &browsers {
            if root.is_empty() { continue; }
            let candidate = Path::new(root).join(suffix);
            if candidate.exists() {
                let path_str = candidate.to_string_lossy().to_string();
                // Avoid duplicates
                if !found.contains(&path_str) {
                    found.push(path_str);
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        let candidates = [
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ];
        for c in &candidates {
            if Path::new(c).exists() {
                found.push(c.to_string());
            }
        }
    } else {
        let candidates = [
            "/usr/bin/microsoft-edge",
            "/usr/bin/google-chrome",
            "/usr/bin/chromium-browser",
            "/usr/bin/chromium",
            "/usr/bin/brave-browser",
        ];
        for c in &candidates {
            if Path::new(c).exists() {
                found.push(c.to_string());
            }
        }
    }

    found
}

/// Get the user data directory path for persistent sessions.
pub fn get_user_data_dir(auth_state_path: &str) -> PathBuf {
    let path = Path::new(auth_state_path);
    let dir = path.parent().unwrap_or(Path::new(".auth"));
    dir.join("browser-data")
}

/// Launch a browser instance.
pub async fn launch_browser(executable_path: &str, headless: bool, user_data_dir: Option<&PathBuf>) -> anyhow::Result<LaunchedBrowser> {
    let mut config_builder = BrowserConfig::builder()
        .chrome_executable(executable_path);
    
    // chromiumoxide is headless by default, with_head() makes it headed
    if !headless {
        config_builder = config_builder.with_head();
    }
    
    // Use provided user_data_dir or generate a unique temp directory.
    // This prevents session restore from previous runs.
    let temp_dir: Option<PathBuf>;
    if let Some(data_dir) = user_data_dir {
        let _ = std::fs::create_dir_all(data_dir);
        let abs_path = std::fs::canonicalize(data_dir)
            .unwrap_or_else(|_| data_dir.clone());
        config_builder = config_builder.user_data_dir(&abs_path);
        temp_dir = None; // Not a temp dir, don't clean up
    } else {
        // Generate unique temp dir to avoid session restore issues
        let unique_dir = std::env::temp_dir().join(format!("openape-browser-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&unique_dir);
        temp_dir = Some(unique_dir.clone());
        config_builder = config_builder.user_data_dir(&unique_dir);
    }
    
    // Add stability flags for Windows Edge compatibility
    #[cfg(windows)]
    {
        config_builder = config_builder
            .arg("--disable-gpu")
            .arg("--disable-software-rasterizer")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-translate")
            .arg("--disable-features=TranslateUI")
            // Prevent session restore (opening previous tabs)
            .arg("--disable-session-crashed-bubble")
            .arg("--disable-infobars")
            .arg("--noerrdialogs")
            .arg("--hide-crash-restore-bubble");
    }
    #[cfg(not(windows))]
    {
        config_builder = config_builder
            .no_sandbox()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-session-crashed-bubble");
    }
    
    let config = config_builder.build()
        .map_err(|e| anyhow::anyhow!("Failed to build browser config: {}", e))?;

    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to launch browser: {}", e))?;
    
    // Spawn the handler to process browser events
    let handle = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {
            // Process events - needed to keep browser responsive
        }
    });

    let page = browser.new_page("about:blank")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create page: {}", e))?;

    Ok(LaunchedBrowser {
        browser,
        page,
        handler: handle,
        temp_dir,
    })
}

/// Close browser and cleanup.
pub async fn close_browser(launched: LaunchedBrowser) {
    let LaunchedBrowser { mut browser, handler, temp_dir, .. } = launched;
    let _ = browser.close().await;
    handler.abort();
    
    // Clean up temp directory
    if let Some(dir) = temp_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Cookie structure for serialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub expires: Option<f64>,
}

/// Convert cookies to a Cookie header string for HTTP requests.
pub fn cookies_to_cookie_header(cookies: &[Cookie], target_url: &str) -> String {
    let is_https = target_url.starts_with("https://");
    // Extract host from URL
    let host = target_url
        .strip_prefix("https://").or_else(|| target_url.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("");

    cookies
        .iter()
        .filter(|c| {
            let cookie_domain = c.domain.trim_start_matches('.');
            host.ends_with(cookie_domain) || host == cookie_domain
        })
        .filter(|c| {
            if c.secure && !is_https { return false; }
            true
        })
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Get cookies from a page.
pub async fn get_cookies(page: &Page) -> anyhow::Result<Vec<Cookie>> {
    let cdp_cookies = page.get_cookies()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get cookies: {}", e))?;
    
    Ok(cdp_cookies.into_iter().map(|c| Cookie {
        name: c.name,
        value: c.value,
        domain: c.domain,
        path: c.path,
        secure: c.secure,
        http_only: c.http_only,
        expires: Some(c.expires),
    }).collect())
}

/// Set cookies on a page.
pub async fn set_cookies(page: &Page, cookies: &[Cookie]) -> anyhow::Result<()> {
    use chromiumoxide::cdp::browser_protocol::network::CookieParam;
    let params: Vec<CookieParam> = cookies.iter().map(|c| {
        CookieParam::builder()
            .name(&c.name)
            .value(&c.value)
            .domain(&c.domain)
            .path(&c.path)
            .secure(c.secure)
            .http_only(c.http_only)
            .build()
            .unwrap()
    }).collect();
    page.set_cookies(params)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set cookies: {}", e))?;
    Ok(())
}
