//! Playwright browser lifecycle management.

use playwright_rs::{Playwright, Browser, BrowserContext, Page, LaunchOptions};
use std::path::Path;

/// Result of launching an authenticated session.
pub struct LaunchedBrowser {
    pub playwright: Playwright,
    pub browser: Browser,
    pub context: BrowserContext,
    pub page: Page,
}

/// Find a Chromium-based browser executable on the system.
/// Priority: Edge > Chrome > Brave
pub fn find_browser_path() -> anyhow::Result<String> {
    if cfg!(target_os = "windows") {
        let program_files = std::env::var("PROGRAMFILES").unwrap_or_default();
        let program_files_x86 = std::env::var("PROGRAMFILES(X86)").unwrap_or_default();
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();

        let browsers = [
            (program_files.clone(), r"Microsoft\Edge\Application\msedge.exe"),
            (program_files_x86.clone(), r"Microsoft\Edge\Application\msedge.exe"),
            (local_app_data.clone(), r"Microsoft\Edge\Application\msedge.exe"),
            (program_files.clone(), r"Google\Chrome\Application\chrome.exe"),
            (program_files_x86.clone(), r"Google\Chrome\Application\chrome.exe"),
            (program_files, r"BraveSoftware\Brave-Browser\Application\brave.exe"),
        ];

        for (root, suffix) in &browsers {
            let candidate = Path::new(root).join(suffix);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
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
                return Ok(c.to_string());
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
                return Ok(c.to_string());
            }
        }
    }

    anyhow::bail!("No browser found (Edge/Chrome/Brave). Please install one.")
}

/// Launch a Playwright + browser instance.
pub async fn launch_playwright(executable_path: &str, headless: bool) -> anyhow::Result<LaunchedBrowser> {
    let playwright = Playwright::launch()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to launch Playwright: {}", e))?;

    let launch_opts = LaunchOptions::new()
        .headless(headless)
        .executable_path(executable_path.to_string());

    let browser = playwright
        .chromium()
        .launch_with_options(launch_opts)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to launch browser: {}", e))?;

    let context = browser
        .new_context()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create context: {}", e))?;

    let page = context
        .new_page()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create page: {}", e))?;

    Ok(LaunchedBrowser { playwright, browser, context, page })
}

/// Safely close browser and context sequentially.
/// Playwright is dropped automatically (implements Drop).
pub async fn close_browser(launched: LaunchedBrowser) {
    let LaunchedBrowser { browser, context, .. } = launched;
    let _ = context.close().await;
    let _ = browser.close().await;
}
