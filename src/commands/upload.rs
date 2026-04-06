use anyhow::Result;
use crate::Cli;
use crate::moodle::upload::upload_file_api;
use super::ApiCtx;

pub async fn run(cmd: &crate::UploadCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::UploadCommands::File { file_path, filename } => {
            let path_str = file_path.to_string_lossy();
            ctx.log.info(&format!("Uploading: {}", path_str));

            let draft_id = upload_file_api(
                &ctx.client, &ctx.session,
                &path_str,
                None,
                filename.as_deref(),
                None,
            ).await?;

            ctx.log.success(&format!("Uploaded successfully! Draft item ID: {}", draft_id));
            ctx.log.info("Use this draft ID with 'assignments submit --file-id <ID>'");

            let result = serde_json::json!({ "draft_item_id": draft_id });
            crate::output::format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}
