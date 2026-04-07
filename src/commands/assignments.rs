use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::assignment::{get_assignments_by_courses_api, get_submission_status_api, save_submission_api};
use crate::moodle::upload::upload_file_api;
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use super::{ApiCtx, level_to_classification};

pub async fn run(cmd: &crate::AssignmentsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::AssignmentsCommands::List { course_id } => {
            let assignments = get_assignments_by_courses_api(&ctx.client, &ctx.session, &[*course_id]).await?;

            let items: Vec<serde_json::Value> = assignments.iter().map(|a| serde_json::json!({
                "id": a.id,
                "cmid": a.cmid,
                "name": a.name,
                "url": a.url,
                "duedate": format_moodle_date(a.duedate),
                "cutoffdate": format_moodle_date(a.cutoffdate),
                "allow_from": format_moodle_date(a.allow_submissions_from_date),
            })).collect();

            ctx.log.info(&format!("Found {} assignments", items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::AssignmentsCommands::ListAll { level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let assignments = get_assignments_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            let course_map: std::collections::HashMap<u64, &str> = courses.iter()
                .map(|c| (c.id, c.fullname.as_str()))
                .collect();

            let items: Vec<serde_json::Value> = assignments.iter().map(|a| serde_json::json!({
                "id": a.id,
                "cmid": a.cmid,
                "course_name": course_map.get(&a.course_id).copied().unwrap_or("Unknown"),
                "name": a.name,
                "url": a.url,
                "duedate": format_moodle_date(a.duedate),
                "cutoffdate": format_moodle_date(a.cutoffdate),
            })).collect();

            ctx.log.info(&format!("Found {} assignments across {} courses", items.len(), courses.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::AssignmentsCommands::Status { assignment_id } => {
            let status = get_submission_status_api(&ctx.client, &ctx.session, *assignment_id).await?;

            let result = serde_json::json!({
                "submitted": status.submitted,
                "graded": status.graded,
                "grader": status.grader,
                "grade": status.grade,
                "feedback": status.feedback,
                "last_modified": format_moodle_date(status.last_modified),
                "files": status.extensions.iter().map(|f| serde_json::json!({
                    "id": f.id,
                    "filename": f.filename,
                    "filesize": f.filesize,
                })).collect::<Vec<_>>(),
            });

            format_and_output(&[result], ctx.output, None);
        }

        crate::AssignmentsCommands::Submit { assignment_id, text, file_id, file } => {
            // Upload file if path is given
            let effective_file_id = if let Some(path) = file {
                let path_str = path.to_string_lossy();
                ctx.log.info(&format!("Uploading file: {}", path_str));
                let draft_id = upload_file_api(
                    &ctx.client, &ctx.session,
                    &path_str, None, None, None,
                ).await?;
                ctx.log.success(&format!("Uploaded file, draft ID: {}", draft_id));
                Some(draft_id)
            } else {
                *file_id
            };

            save_submission_api(
                &ctx.client, &ctx.session,
                *assignment_id,
                text.as_deref(),
                effective_file_id,
            ).await?;

            ctx.log.success("Assignment submitted successfully!");
            let result = serde_json::json!({
                "action": "submit",
                "assignment_id": *assignment_id,
                "success": true,
                "file_id": effective_file_id,
                "has_text": text.is_some(),
            });
            format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}
