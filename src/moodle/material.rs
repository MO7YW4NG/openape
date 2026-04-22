use super::client::moodle_api_call;
use crate::moodle_args;
use crate::auth::browser::{find_browser_paths, launch_browser, close_browser, set_cookies, Cookie};
use super::types::{SessionInfo};
use super::types::ResourceModule;
use reqwest::Client;
use chromiumoxide::cdp::browser_protocol::network::EventRequestWillBeSent;
use chromiumoxide::Page;
use futures::StreamExt;

/// Get resources (resource + pdfannotator) from course contents via core_course_get_contents.
pub async fn get_course_contents_resources(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
) -> anyhow::Result<Vec<ResourceModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("courseid" => course_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_course_get_contents", &args).await?;

    let sections = data.as_array().cloned().unwrap_or_default();
    let mut resources = Vec::new();

    for section in &sections {
        let modules = section.get("modules").and_then(|m| m.as_array()).cloned().unwrap_or_default();
        for module in &modules {
            let modname = module.get("modname").and_then(|v| v.as_str()).unwrap_or("");
            if modname != "resource" && modname != "pdfannotator" { continue; }

            let cmid = module.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = module.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let contextid = module.get("contextid").and_then(|v| v.as_u64());
            let view_url = module.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();

            if modname == "pdfannotator" {
                // Rule-based fallback URL for token-auth download
                let fallback_url = if let Some(ctxid) = contextid {
                    let filename = format!("{}.pdf", &name);
                    let encoded_name = crate::utils::percent_encode(&filename);
                    format!("{}/webservice/pluginfile.php/{}/mod_pdfannotator/content/0/{}",
                        session.moodle_base_url, ctxid, encoded_name)
                } else {
                    String::new()
                };

                resources.push(ResourceModule {
                    cmid: cmid.to_string(),
                    name,
                    url: fallback_url,
                    view_url: Some(view_url),
                    course_id,
                    mod_type: "pdfannotator".to_string(),
                    contextid,
                    mimetype: Some("application/pdf".to_string()),
                    filesize: None,
                    modified: None,
                });
            } else {
                // Standard resource — try contents first, fall back to view URL
                let file_url = module.get("contents").and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|f| f.get("fileurl"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&view_url)
                    .to_string();

                let first_file = module.get("contents").and_then(|c| c.as_array())
                    .and_then(|arr| arr.first());
                let mimetype = first_file.and_then(|f| f.get("mimetype")).and_then(|v| v.as_str()).map(String::from);
                let filesize = first_file.and_then(|f| f.get("filesize")).and_then(|v| v.as_u64());

                resources.push(ResourceModule {
                    cmid: cmid.to_string(),
                    name,
                    url: file_url,
                    view_url: None,
                    course_id,
                    mod_type: "resource".to_string(),
                    contextid,
                    mimetype,
                    filesize,
                    modified: None,
                });
            }
        }
    }

    Ok(resources)
}

/// Mark a resource as viewed via WS API (for completionview-type tracking).
pub async fn view_resource_api(
    client: &Client,
    session: &SessionInfo,
    instance_id: u64,
) -> anyhow::Result<bool> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("resourceid" => instance_id);
    let result = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_resource_view_resource", &args).await?;

    Ok(result.get("status").and_then(|v| v.as_bool()).unwrap_or(result.is_null()))
}

/// Get incomplete activity completion statuses for a course.
pub async fn get_incomplete_completions(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
    userid: u64,
) -> anyhow::Result<Vec<IncompleteCompletion>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("courseid" => course_id, "userid" => userid);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "core_completion_get_activities_completion_status", &args).await?;

    let statuses = data.get("statuses").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    Ok(statuses.into_iter().filter_map(|s| {
        let hascompletion = s.get("hascompletion").and_then(|v| v.as_bool()).unwrap_or(false);
        let overall = s.get("isoverallcomplete").and_then(|v| v.as_bool()).unwrap_or(false);
        if !hascompletion || overall { return None; }

        let cmid = s.get("cmid").and_then(|v| v.as_u64()).unwrap_or(0);
        let instance = s.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
        let modname = s.get("modname").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // Extract completion rule
        let rule = s.get("details").and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|d| d.get("rulename").and_then(|v| v.as_str()))
            .map(String::from);

        Some(IncompleteCompletion { cmid, instance, modname, name, rule })
    }).collect())
}

/// Info about an incomplete activity's completion tracking.
pub struct IncompleteCompletion {
    pub cmid: u64,
    pub instance: u64,
    pub modname: String,
    pub name: String,
    pub rule: Option<String>,
}

/// Resolve actual PDF download URLs for pdfannotator modules using headless browser.
/// Visits each view page, intercepts network requests to capture the pluginfile PDF URL.
/// Returns a map of cmid -> resolved_url.
pub async fn resolve_pdfannotator_urls(
    pdfannotators: &[(String, String, String)], // (cmid, name, view_url)
    auth_cookies: &[Cookie],
    base_url: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    if pdfannotators.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let browser_candidates = find_browser_paths();
    if browser_candidates.is_empty() {
        anyhow::bail!("No browser found for headless PDF URL resolution");
    }

    let mut launched = None;
    for exe in &browser_candidates {
        if let Ok(l) = launch_browser(exe, true, None).await {
            launched = Some(l);
            break;
        }
    }
    let launched = launched.ok_or_else(|| anyhow::anyhow!("Failed to launch headless browser"))?;

    // Navigate to the Moodle domain first (cookies can't be set on about:blank)
    let _ = launched.page.goto(base_url).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    set_cookies(&launched.page, auth_cookies).await?;

    let mut result_map = std::collections::HashMap::new();

    for (cmid, _name, view_url) in pdfannotators {
        if let Some(url) = capture_pdf_url(&launched.page, view_url).await {
            result_map.insert(cmid.clone(), url);
        }
    }

    close_browser(launched).await;
    Ok(result_map)
}

/// Visit a pdfannotator view page and capture the PDF pluginfile URL.
/// Tries HTML source extraction first, then network interception on reload.
/// Returns None if redirected to login or no URL found within timeout.
async fn capture_pdf_url(page: &Page, view_url: &str) -> Option<String> {
    let _ = page.goto(view_url).await;

    let page_url = page.url().await.ok().flatten().unwrap_or_default();

    // Check for login redirect
    if page_url.contains("login") { return None; }

    // Method 1: Extract from page source (iframe/embed/src patterns)
    if let Ok(content) = page.content().await {
        if let Some(url) = extract_pdf_url_from_html(&content) {
            return Some(url);
        }
    }

    // Method 2: Network interception — register listener and reload
    let mut events = page.event_listener::<EventRequestWillBeSent>().await.ok()?;
    let _ = page.reload().await;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }

        match tokio::time::timeout(remaining, events.next()).await {
            Ok(Some(event)) => {
                let req_url = &event.request.url;
                if req_url.contains("pluginfile.php")
                    && req_url.contains("mod_pdfannotator")
                    && req_url.ends_with(".pdf")
                {
                    return Some(req_url.clone());
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }

    None
}

/// Extract a PDF pluginfile URL from HTML content.
fn extract_pdf_url_from_html(html: &str) -> Option<String> {
    // Look for pluginfile.php URLs containing mod_pdfannotator in src, href, or data attributes
    for part in html.split(['"', '\'']) {
        if part.contains("pluginfile.php")
            && part.contains("mod_pdfannotator")
            && part.contains(".pdf")
        {
            // Extract just the URL (may have query params after)
            let url_start = part.find("https://").or_else(|| part.find("http://"))?;
            let url_end = part[url_start..].find(|c: char| "\"' \t>".contains(c))
                .unwrap_or(part.len() - url_start);
            let url = &part[url_start..url_start + url_end];
            if url.ends_with(".pdf") {
                return Some(url.to_string());
            }
        }
    }
    None
}
