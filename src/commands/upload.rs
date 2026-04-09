use anyhow::Result;
use crate::Cli;
use crate::moodle::upload::upload_file_api;
use super::ApiCtx;

pub async fn run(cmd: &crate::UploadCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::UploadCommands::File { file_path, filename } => {
            let path_str = file_path.to_string_lossy();
            ctx.log.info(&format!("Uploading: {}", path_str));

            let file_meta = std::fs::metadata(file_path)?;
            let effective_filename = filename.clone().or_else(|| {
                file_path.file_name().map(|s| s.to_string_lossy().to_string())
            }).unwrap_or_else(|| "upload.bin".to_string());

            let draft_id = upload_file_api(
                &ctx.client, &ctx.session,
                &path_str,
                None,
                filename.as_deref(),
                None,
            ).await?;

            ctx.log.success(&format!("Uploaded successfully! Draft item ID: {}", draft_id));
            ctx.log.info("Use this draft ID with 'assignments submit --file-id <ID>'");

            let result = serde_json::json!({
                "success": true,
                "draft_item_id": draft_id,
                "draft_id": draft_id,
                "filename": effective_filename,
                "source_path": file_path.to_string_lossy(),
                "filesize": file_meta.len(),
                "message": "Use this draft ID for assignment submission or forum posts",
            });
            crate::output::format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}
