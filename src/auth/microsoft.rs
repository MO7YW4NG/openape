use anyhow::Context;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::Page;
use std::time::Duration;

use crate::auth::credentials::StoredCredentials;
use crate::logger::Logger;

const SUBMIT_BUTTON_SELECTOR: &str = "#idSIButton9";
const STAY_SIGNED_IN_YES: &str = "#idSIButton9";
const STAY_SIGNED_IN_ACCEPT: &str = "#idBtn_Accept";

const _EMAIL_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const PASSWORD_WAIT_TIMEOUT: Duration = Duration::from_secs(8);

/// Check if an element exists in the live DOM via JavaScript.
async fn js_element_exists(page: &Page, selector: &str) -> bool {
    let js = format!(
        "document.querySelector('{}') !== null",
        selector.replace('\'', "\\'")
    );
    page.evaluate(js)
        .await
        .ok()
        .and_then(|v| v.value().and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

async fn js_element_visible(page: &Page, selector: &str) -> bool {
    let escaped_selector = selector.replace('\'', "\\'");
    let js = format!(
        r#"(function() {{
            const el = document.querySelector('{}');
            if (!el) return false;
            const style = window.getComputedStyle(el);
            const rect = el.getBoundingClientRect();
            return style.display !== 'none'
                && style.visibility !== 'hidden'
                && rect.width > 0
                && rect.height > 0;
        }})()"#,
        escaped_selector
    );
    page.evaluate(js)
        .await
        .ok()
        .and_then(|v| v.value().and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// Fill a form input via JavaScript: focus, set value, dispatch events.
async fn js_fill_input(
    page: &Page,
    selector: &str,
    value: &str,
    _log: &Logger,
) -> anyhow::Result<()> {
    let escaped_selector = selector.replace('\'', "\\'");
    let escaped_value = value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n");
    let js = format!(
        r#"(function() {{
            const el = document.querySelector('{}');
            if (!el) return 'NOT_FOUND';
            el.focus();
            const previousValue = el.value;
            const proto = Object.getPrototypeOf(el);
            const valueSetter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
            if (valueSetter) {{
                valueSetter.call(el, '{}');
            }} else {{
                el.value = '{}';
            }}
            if (el._valueTracker) {{
                el._valueTracker.setValue(previousValue);
            }}
            el.dispatchEvent(new InputEvent('input', {{bubbles: true, cancelable: true, inputType: 'insertText', data: '{}'}}));
            el.dispatchEvent(new Event('input', {{bubbles: true}}));
            el.dispatchEvent(new Event('change', {{bubbles: true}}));
            el.dispatchEvent(new Event('propertychange', {{bubbles: true}}));
            el.dispatchEvent(new KeyboardEvent('keydown', {{bubbles: true, key: 'a'}}));
            el.dispatchEvent(new KeyboardEvent('keypress', {{bubbles: true, key: 'a'}}));
            el.dispatchEvent(new KeyboardEvent('keyup', {{bubbles: true, key: 'a'}}));
            el.blur();
            return 'OK';
        }})()"#,
        escaped_selector, escaped_value, escaped_value, escaped_value
    );
    let result = page
        .evaluate(js)
        .await
        .context("Failed to evaluate fill-input JS")?;
    let status = result
        .value()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    match status.as_str() {
        "OK" => Ok(()),
        "NOT_FOUND" => {
            anyhow::bail!("Element '{}' not found in DOM via JS", selector);
        }
        _ => {
            anyhow::bail!("Unexpected JS fill result: {}", status);
        }
    }
}

async fn cdp_insert_text_input(
    page: &Page,
    selector: &str,
    value: &str,
    _log: &Logger,
) -> anyhow::Result<()> {
    let escaped_selector = selector.replace('\'', "\\'");
    let focus_js = format!(
        r#"(function() {{
            const matches = Array.from(document.querySelectorAll('{}'));
            const el = matches.find((candidate) => {{
                const style = window.getComputedStyle(candidate);
                const rect = candidate.getBoundingClientRect();
                return style.display !== 'none'
                    && style.visibility !== 'hidden'
                    && rect.width > 0
                    && rect.height > 0;
            }}) || matches[0];
            if (!el) return 'NOT_FOUND';
            el.focus();
            el.select();
            return 'OK';
        }})()"#,
        escaped_selector
    );
    let focus_result = page
        .evaluate(focus_js)
        .await
        .context("Failed to focus input before CDP insertText")?;
    if focus_result
        .value()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        != "OK"
    {
        anyhow::bail!("Element '{}' not found for CDP insertText", selector);
    }

    page.execute(InsertTextParams::new(value))
        .await
        .context("Failed to insert text via CDP")?;
    Ok(())
}

/// Click an element via JavaScript.
async fn js_click(page: &Page, selector: &str) -> anyhow::Result<()> {
    let escaped_selector = selector.replace('\'', "\\'");
    let js = format!(
        r#"(function() {{
            const matches = Array.from(document.querySelectorAll('{}'));
            const el = matches.find((candidate) => {{
                const style = window.getComputedStyle(candidate);
                const rect = candidate.getBoundingClientRect();
                return style.display !== 'none'
                    && style.visibility !== 'hidden'
                    && rect.width > 0
                    && rect.height > 0
                    && !candidate.disabled
                    && candidate.getAttribute('aria-disabled') !== 'true';
            }}) || matches[0];
            if (!el) return 'NOT_FOUND';
            el.disabled = false;
            el.removeAttribute('disabled');
            el.setAttribute('aria-disabled', 'false');
            el.click();
            return 'OK';
        }})()"#,
        escaped_selector
    );
    let result = page
        .evaluate(js)
        .await
        .context("Failed to evaluate click JS")?;
    let status = result
        .value()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    match status.as_str() {
        "OK" => Ok(()),
        "NOT_FOUND" => anyhow::bail!("Element '{}' not found in DOM for click via JS", selector),
        _ => anyhow::bail!("Unexpected JS click result: {}", status),
    }
}

/// Wait for a DOM element to appear using JavaScript polling.
async fn js_wait_for_element(
    page: &Page,
    selector: &str,
    fallback: Option<&str>,
    timeout: Duration,
    _log: &Logger,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if js_element_exists(page, selector).await {
            return Ok(());
        }
        if let Some(fb) = fallback {
            if js_element_exists(page, fb).await {
                return Ok(());
            }
        }
        if tokio::time::Instant::now() > deadline {
            let current_url = page.url().await.ok().flatten().unwrap_or_default();
            anyhow::bail!(
                "Timed out waiting for element: {} (URL: {})",
                selector,
                current_url
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Poll for the password input to appear (instead of a fixed sleep after clicking Next).
async fn wait_for_password_page(
    page: &Page,
    base_domain: &str,
    timeout: Duration,
    _log: &Logger,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if js_element_exists(page, "input[name='passwd']").await
            || js_element_exists(page, "input[type='password']").await
        {
            return Ok(());
        }
        if let Ok(Some(url)) = page.url().await {
            if url.contains(base_domain)
                && !url.contains("login")
                && !url.contains("microsoftonline")
            {
                anyhow::bail!("REDIRECTED_BACK");
            }
        }
        if stay_signed_in_selector(page).await.is_some() {
            anyhow::bail!("STAY_SIGNED_IN");
        }
        if tokio::time::Instant::now() > deadline {
            let current_url = page.url().await.ok().flatten().unwrap_or_default();
            anyhow::bail!("Timed out waiting for password page (URL: {})", current_url);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Poll for the email input to appear.
async fn wait_for_email_page(page: &Page, timeout: Duration, _log: &Logger) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if js_element_exists(page, "input[name='loginfmt']").await
            || js_element_exists(page, "input[type='email']").await
        {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            let current_url = page.url().await.ok().flatten().unwrap_or_default();
            anyhow::bail!("Timed out waiting for email input (URL: {})", current_url);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Wait until the page URL stops containing redirect keywords.
#[allow(dead_code)]
async fn wait_for_redirect_settle(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<()> {
    let base_domain = base_url.replace("https://", "").replace("http://", "");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut last_url = String::new();
    let mut stable_count = 0u32;
    loop {
        if let Ok(Some(url)) = page.url().await {
            // Success: we're on the Moodle domain and no longer on Microsoft login pages
            if url.contains(&base_domain) && !url.contains("microsoftonline") {
                // Count how many times we've seen the same URL — if stable for 3s, accept it
                if url == last_url {
                    stable_count += 1;
                    if stable_count >= 6 {
                        log.debug(&format!("Page settled on Moodle URL: {}", url));
                        return Ok(());
                    }
                } else {
                    stable_count = 0;
                    log.debug(&format!("Redirect detected, current URL: {}", url));
                    last_url = url.clone();
                }
                // Also accept immediately if it's clearly the Moodle dashboard
                if url.contains("/my/") || url.contains("/course/") {
                    return Ok(());
                }
            } else {
                stable_count = 0;
                if url != last_url {
                    log.debug(&format!("Redirect detected, current URL: {}", url));
                    last_url = url.clone();
                }
            }
        }
        if tokio::time::Instant::now() > deadline {
            let final_url = page.url().await.ok().flatten().unwrap_or_default();
            anyhow::bail!(
                "Timed out waiting for redirect to settle. Final URL: {}",
                final_url
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn auth_input_visible(page: &Page) -> bool {
    js_element_visible(page, "input[name='loginfmt']").await
        || js_element_visible(page, "input[type='email']").await
        || js_element_visible(page, "input[name='passwd']").await
        || js_element_visible(page, "input[type='password']").await
}

async fn stay_signed_in_selector(page: &Page) -> Option<&'static str> {
    if auth_input_visible(page).await {
        return None;
    }

    if js_element_visible(page, STAY_SIGNED_IN_ACCEPT).await
        || js_element_exists(page, STAY_SIGNED_IN_ACCEPT).await
    {
        return Some(STAY_SIGNED_IN_ACCEPT);
    }

    if js_element_visible(page, STAY_SIGNED_IN_YES).await
        || (page_contains_text(page, "Stay signed in").await
            && js_element_exists(page, STAY_SIGNED_IN_YES).await)
    {
        return Some(STAY_SIGNED_IN_YES);
    }

    None
}

async fn page_contains_text(page: &Page, text: &str) -> bool {
    let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
    let js = format!(
        r#"(document.body && document.body.innerText && document.body.innerText.includes('{}'))"#,
        escaped_text
    );
    page.evaluate(js)
        .await
        .ok()
        .and_then(|v| v.value().and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

async fn sign_in_button_ready(page: &Page) -> bool {
    let js = format!(
        r#"(function() {{
            const matches = Array.from(document.querySelectorAll('{}'));
            return matches.some((el) => {{
                const style = window.getComputedStyle(el);
                const rect = el.getBoundingClientRect();
                const label = `${{el.value || ''}} ${{el.innerText || ''}} ${{el.textContent || ''}}`;
                return style.display !== 'none'
                    && style.visibility !== 'hidden'
                    && rect.width > 0
                    && rect.height > 0
                    && !el.disabled
                    && el.getAttribute('aria-disabled') !== 'true'
                    && label.includes('Sign in');
            }});
        }})()"#,
        SUBMIT_BUTTON_SELECTOR.replace('\'', "\\'")
    );
    page.evaluate(js)
        .await
        .ok()
        .and_then(|v| v.value().and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

async fn wait_for_sign_in_button_ready(page: &Page, timeout: Duration) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if sign_in_button_ready(page).await {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn verify_moodle_dashboard(page: &Page, base_url: &str, log: &Logger) -> anyhow::Result<()> {
    let base_domain = base_url.replace("https://", "").replace("http://", "");

    log.info("Verifying login by navigating to Moodle dashboard...");
    let dashboard_url = format!("{}/my/", base_url);
    let _ = page.goto(&dashboard_url).await;
    let _ = page.wait_for_navigation().await;
    tokio::time::sleep(Duration::from_millis(250)).await;

    if let Ok(Some(url)) = page.url().await {
        if url.contains(&base_domain) && !url.contains("login") && !url.contains("microsoftonline")
        {
            log.success("Headless login completed successfully.");
            return Ok(());
        }
    }

    let current_url = page.url().await.ok().flatten().unwrap_or_default();
    anyhow::bail!(
        "Authentication failed. Still on login page after all steps. URL: {}",
        current_url
    )
}

async fn handle_stay_signed_in_prompt(page: &Page, log: &Logger) -> anyhow::Result<bool> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(selector) = stay_signed_in_selector(page).await {
            log.info("Found 'Stay signed in?' prompt, clicking Yes...");
            js_click(page, selector)
                .await
                .context("Failed to click Stay signed in button")?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            return Ok(true);
        }

        if let Ok(Some(url)) = page.url().await {
            if !url.contains("login.microsoftonline.com") {
                break;
            }
        }

        if tokio::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    Ok(false)
}

async fn complete_password_login(
    page: &Page,
    base_url: &str,
    password: &str,
    log: &Logger,
) -> anyhow::Result<()> {
    log.info("Entering password...");
    js_wait_for_element(
        page,
        "input[name='passwd']",
        Some("input[type='password']"),
        PASSWORD_WAIT_TIMEOUT,
        log,
    )
    .await
    .context(
        "Timed out waiting for password input (MFA may be required, or the account may not exist)",
    )?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    if cdp_insert_text_input(page, "input[name='passwd']", password, log)
        .await
        .is_err()
    {
        cdp_insert_text_input(page, "input[type='password']", password, log).await?;
    }
    wait_for_sign_in_button_ready(page, Duration::from_secs(5)).await?;

    log.info("Clicking Sign in...");
    js_wait_for_element(
        page,
        SUBMIT_BUTTON_SELECTOR,
        None,
        Duration::from_secs(5),
        log,
    )
    .await?;
    js_click(page, SUBMIT_BUTTON_SELECTOR)
        .await
        .context("Failed to click Sign in button")?;

    let _ = handle_stay_signed_in_prompt(page, log).await?;
    verify_moodle_dashboard(page, base_url, log).await
}

/// Perform automated Microsoft OAuth login by filling form fields via JavaScript.
pub async fn perform_headless_login(
    page: &Page,
    base_url: &str,
    credentials: &StoredCredentials,
    log: &Logger,
) -> anyhow::Result<()> {
    log.info("Starting headless Microsoft OAuth login...");

    let base_domain = base_url.replace("https://", "").replace("http://", "");

    // Step 1: Navigate to Moodle login page to trigger SSO redirect
    page.goto(&format!("{}/login/index.php", base_url))
        .await
        .context("Failed to navigate to login page")?;
    let _ = page.wait_for_navigation().await;
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Wait for redirect to Microsoft, or check if we need to click a login button on the Moodle page
    let redirect_deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        if let Ok(Some(url)) = page.url().await {
            if url.contains(&base_domain)
                && !url.contains("login")
                && !url.contains("microsoftonline")
            {
                log.success("Already logged in (session valid after navigation).");
                return Ok(());
            }
            if url.contains("microsoftonline") {
                log.info(&format!("Reached Microsoft login page: {}", url));
                break;
            }
        }
        if tokio::time::Instant::now() > redirect_deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // If we're still on the Moodle login page, try to find and click the SSO login button
    if !page
        .url()
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .contains("microsoftonline")
    {
        if let Ok(Some(url)) = page.url().await {
            log.info(&format!(
                "Still on Moodle page: {}. Looking for SSO login button...",
                url
            ));
        }

        let sso_selectors: &[&str] = &[
            "a[href*='auth/oidc']",
            "a[href*='auth/oid']",
            ".login-oidc a",
            "#region-main a[href*='auth']",
            "form[action*='auth/oidc'] button",
            ".btn-login",
            "input[type='submit'][name='login']",
            "button[type='submit']",
            "#loginbtn",
            "form#login input[type='submit']",
        ];

        for selector in sso_selectors {
            if js_element_exists(page, selector).await {
                log.info(&format!(
                    "Found SSO button with selector: {}, clicking...",
                    selector
                ));
                let _ = js_click(page, selector).await;
                let sso_deadline = tokio::time::Instant::now() + Duration::from_secs(6);
                loop {
                    if let Ok(Some(url)) = page.url().await {
                        if url.contains("microsoftonline") {
                            break;
                        }
                        if url.contains(&base_domain) && !url.contains("login") {
                            break;
                        }
                    }
                    if tokio::time::Instant::now() > sso_deadline {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                break;
            }
        }

        let retry_deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            if let Ok(Some(url)) = page.url().await {
                if url.contains(&base_domain)
                    && !url.contains("login")
                    && !url.contains("microsoftonline")
                {
                    log.success("Already logged in (session valid after SSO button click).");
                    return Ok(());
                }
                if url.contains("microsoftonline") {
                    log.info(&format!("Reached Microsoft login page: {}", url));
                    break;
                }
            }
            if tokio::time::Instant::now() > retry_deadline {
                let current_url = page.url().await.ok().flatten().unwrap_or_default();
                anyhow::bail!(
                    "Timed out waiting for redirect to Microsoft login page. Current URL: {}",
                    current_url
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    // Step 2: Poll for the Microsoft page to be interactive
    let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        if js_element_exists(page, "input[name='loginfmt']").await
            || js_element_exists(page, "input[type='email']").await
            || js_element_exists(page, "[data-test-id]").await
            || js_element_exists(page, "#otherTileText").await
            || js_element_exists(page, "[role='link']").await
        {
            break;
        }
        if tokio::time::Instant::now() > ready_deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    // Check if we're on the "Pick an account" page or the email input page
    let on_email_page = js_element_exists(page, "input[name='loginfmt']").await
        || js_element_exists(page, "input[type='email']").await;
    let has_account_tiles = !on_email_page && page.evaluate(
        r#"document.querySelectorAll('[data-test-id], [role="link"], [role="button"]').length > 0"#
    ).await.ok().and_then(|v| v.value().and_then(|v| v.as_bool())).unwrap_or(false);

    if !on_email_page && has_account_tiles {
        // Account picker page
        log.info("Account picker page detected. Looking for saved account...");
        let email = credentials.email();
        let click_js = format!(
            r#"(function() {{
                const tiles = document.querySelectorAll('[data-test-id]');
                for (const tile of tiles) {{
                    if (tile.textContent && tile.textContent.includes('{}')) {{
                        tile.click();
                        return 'TILE_CLICKED';
                    }}
                }}
                const clickables = document.querySelectorAll('[role="link"], [role="button"], a');
                for (const el of clickables) {{
                    if (el.textContent && el.textContent.includes('{}')) {{
                        el.click();
                        return 'LINK_CLICKED';
                    }}
                }}
                const divs = document.querySelectorAll('div, tr, td');
                for (const el of divs) {{
                    const text = el.textContent ? el.textContent.trim() : '';
                    if (text === '{}' || (text.startsWith('{}') && text.length < '{}'.length + 20)) {{
                        el.click();
                        return 'DIV_CLICKED';
                    }}
                }}
                return 'NOT_FOUND';
            }})()"#,
            email, email, email, email, email
        );
        let clicked = page
            .evaluate(click_js)
            .await
            .ok()
            .and_then(|v| v.value().and_then(|v| v.as_str().map(String::from)))
            .unwrap_or_default();

        if clicked != "NOT_FOUND" {
            log.info(&format!(
                "Clicked saved account tile for {} ({})",
                email, clicked
            ));

            // Poll for next state instead of fixed sleep
            let after_click_deadline = tokio::time::Instant::now() + Duration::from_secs(6);
            loop {
                if let Ok(Some(url)) = page.url().await {
                    if url.contains(&base_domain)
                        && !url.contains("login")
                        && !url.contains("microsoftonline")
                    {
                        log.success("Login completed via saved account tile.");
                        return Ok(());
                    }
                }
                if js_element_exists(page, "input[name='passwd']").await
                    || js_element_exists(page, "input[type='password']").await
                    || js_element_exists(page, "input[name='loginfmt']").await
                    || js_element_exists(page, "input[type='email']").await
                {
                    break;
                }
                if tokio::time::Instant::now() > after_click_deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }

            // Check if password input appeared (skip email step)
            let on_password = js_element_exists(page, "input[name='passwd']").await
                || js_element_exists(page, "input[type='password']").await;
            let on_email = js_element_exists(page, "input[name='loginfmt']").await
                || js_element_exists(page, "input[type='email']").await;

            if on_password {
                log.info("Password page reached after account tile click.");
                complete_password_login(page, base_url, &credentials.password, log).await?;
                return Ok(());
            } else if on_email {
                // Account tile click took us to email input — fill it
                log.info(&format!("Entering email: {}...", credentials.email()));
                if js_fill_input(page, "input[name='loginfmt']", &credentials.email(), log)
                    .await
                    .is_err()
                {
                    js_fill_input(page, "input[type='email']", &credentials.email(), log).await?;
                }
            }
        } else {
            // Couldn't find our email tile — click "Use another account"
            log.info("Account tile not found. Clicking 'Use another account'...");
            let use_another_js = r#"(function() {
                const otherTile = document.querySelector('#otherTileText');
                if (otherTile) { otherTile.click(); return 'CLICKED_OTHER'; }
                const links = document.querySelectorAll('a, [role="link"], [role="button"]');
                for (const link of links) {
                    if (link.textContent && (link.textContent.includes('Use another') || link.textContent.includes('another account') || link.textContent.includes('Sign in with a'))) {
                        link.click();
                        return 'CLICKED_LINK';
                    }
                }
                return 'NOT_FOUND';
            })()"#;
            let use_result = page
                .evaluate(use_another_js)
                .await
                .ok()
                .and_then(|v| v.value().and_then(|v| v.as_str().map(String::from)))
                .unwrap_or_default();

            if use_result != "NOT_FOUND" {
                let _ = wait_for_email_page(page, Duration::from_secs(6), log).await;
            }
        }
    }

    // Step 3: Fill email (if we're on the email input page)
    let on_email_page = js_element_exists(page, "input[name='loginfmt']").await
        || js_element_exists(page, "input[type='email']").await;
    if on_email_page {
        log.info(&format!("Entering email: {}...", credentials.email()));
        if js_fill_input(page, "input[name='loginfmt']", &credentials.email(), log)
            .await
            .is_err()
        {
            js_fill_input(page, "input[type='email']", &credentials.email(), log).await?;
        }

        // Step 4: Click "Next" then poll for password page
        js_wait_for_element(
            page,
            SUBMIT_BUTTON_SELECTOR,
            None,
            Duration::from_secs(5),
            log,
        )
        .await?;
        js_click(page, SUBMIT_BUTTON_SELECTOR)
            .await
            .context("Failed to click Next button")?;

        match wait_for_password_page(page, &base_domain, Duration::from_secs(8), log).await {
            Ok(()) => {}
            Err(e) if e.to_string().contains("REDIRECTED_BACK") => {
                log.success("Login completed (redirected back to Moodle during password wait).");
                return Ok(());
            }
            Err(e) if e.to_string().contains("STAY_SIGNED_IN") => {
                let _ = handle_stay_signed_in_prompt(page, log).await?;
                verify_moodle_dashboard(page, base_url, log).await?;
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }

    complete_password_login(page, base_url, &credentials.password, log).await
}
