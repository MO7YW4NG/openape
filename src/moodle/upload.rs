use super::course::{get_site_info, get_user_context_id};
use super::types::SessionInfo;
use reqwest::Client;

/// Generate a draft item ID from timestamp.
pub fn generate_draft_item_id() -> u64 {
    (chrono::Utc::now().timestamp() % 100_000_000) as u64
}

/// Upload a file to Moodle draft area.
pub async fn upload_file_api(
    client: &Client,
    session: &SessionInfo,
    file_path: &str,
    draft_id: Option<u64>,
    filename: Option<&str>,
    filepath: Option<&str>,
) -> anyhow::Result<u64> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let site_info = get_site_info(client, session).await?;
    let draft_item_id = draft_id.unwrap_or_else(generate_draft_item_id);
    let file_name = filename.unwrap_or_else(|| {
        std::path::Path::new(file_path).file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    });
    let file_bytes = tokio::fs::read(file_path).await?;
    let user_context_id = get_user_context_id(site_info.userid);

    let form = reqwest::multipart::Form::new()
        .text("token", ws_token.clone())
        .part("file", reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string()))
        .text("filepath", filepath.unwrap_or("/").to_string())
        .text("itemid", draft_item_id.to_string())
        .text("contextid", user_context_id.to_string())
        .text("component", "user".to_string())
        .text("filearea", "draft".to_string())
        .text("qformat", "");

    let url = format!("{}/webservice/upload.php", session.moodle_base_url);
    let resp = client.post(&url).multipart(form).send().await?;
    let result: serde_json::Value = resp.json().await?;

    if result.get("error").is_some() {
        let msg = result.get("message")
            .or_else(|| result.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("Upload failed");
        anyhow::bail!("{}", msg);
    }

    Ok(draft_item_id)
}
