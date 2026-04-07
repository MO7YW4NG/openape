use super::client::moodle_api_call;
use super::course::get_site_info;
use super::types::{AssignmentModule, DraftFile, SubmissionStatus, SessionInfo};
use crate::moodle_args;
use reqwest::Client;
use serde_json::Value;

/// Get assignments by course IDs via WS API.
pub async fn get_assignments_by_courses_api(
    client: &Client,
    session: &SessionInfo,
    course_ids: &[u64],
) -> anyhow::Result<Vec<AssignmentModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    if course_ids.is_empty() { return Ok(Vec::new()); }

    let course_ids_json: Vec<Value> = course_ids.iter().map(|id| serde_json::json!(*id)).collect();
    let args = moodle_args!("courseids" => course_ids_json);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_assign_get_assignments", &args).await?;

    let courses = data.get("courses").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let mut assignments = Vec::new();

    for course in &courses {
        let course_id = course.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let assign_arr = course.get("assignments").and_then(|a| a.as_array()).cloned().unwrap_or_default();
        for a in &assign_arr {
            assignments.push(AssignmentModule {
                id: a.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                cmid: a.get("cmid").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
                name: a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                url: a.get("viewurl").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                course_id,
                duedate: a.get("duedate").and_then(|v| v.as_i64()),
                cutoffdate: a.get("cutoffdate").and_then(|v| v.as_i64()),
                allow_submissions_from_date: a.get("allowsubmissionsfromdate").and_then(|v| v.as_i64()),
                grading_due_date: a.get("gradingduedate").and_then(|v| v.as_i64()),
                late_submission: a.get("latesubmissions").and_then(|v| v.as_bool()),
                extension_due_date: a.get("extensionduedate").and_then(|v| v.as_i64()),
            });
        }
    }

    Ok(assignments)
}

/// Get assignment submission status.
pub async fn get_submission_status_api(
    client: &Client,
    session: &SessionInfo,
    assignment_id: u64,
) -> anyhow::Result<SubmissionStatus> {
    let site_info = get_site_info(client, session).await?;
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("assignid" => assignment_id, "userid" => site_info.userid);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_assign_get_submission_status", &args).await?;

    let last_attempt = data.get("lastattempt");
    let submission = last_attempt.and_then(|la| la.get("submission"));
    let feedback = data.get("feedback");

    let plugins = submission.and_then(|s| s.get("plugins")).and_then(|p| p.as_array()).cloned().unwrap_or_default();
    let file_plugin = plugins.iter().find(|p| p.get("type").and_then(|v| v.as_str()) == Some("file"));
    let extensions: Vec<super::types::DraftFile> = file_plugin
        .and_then(|fp| fp.get("fileareas").and_then(|fa| fa.as_array()))
        .map(|fas| fas.iter()
            .flat_map(|fa| fa.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default())
            .filter_map(|f| Some(DraftFile {
                id: f.get("id")?.as_u64()?,
                filename: f.get("filename")?.as_str()?.to_string(),
                filesize: f.get("filesize")?.as_u64()?,
            }))
            .collect())
        .unwrap_or_default();

    let comments_plugin = feedback
        .and_then(|fb| fb.get("plugins").and_then(|p| p.as_array()))
        .and_then(|plugins| {
            plugins.iter().find(|p| p.get("type").and_then(|v| v.as_str()) == Some("comments"))
        })
        .and_then(|cp| cp.get("editorfields").and_then(|ef| ef.as_array()))
        .and_then(|fields| {
            fields.iter().find(|f| f.get("name").and_then(|v| v.as_str()) == Some("comments"))
                .and_then(|f| f.get("text").and_then(|v| v.as_str()).map(String::from))
        });

    Ok(SubmissionStatus {
        submitted: submission.and_then(|s| s.get("status")).and_then(|v| v.as_str()) == Some("submitted"),
        graded: last_attempt.and_then(|la| la.get("gradingstatus")).and_then(|v| v.as_str()) == Some("graded"),
        grader: feedback.and_then(|fb| fb.get("gradername")).and_then(|v| v.as_str()).map(String::from),
        grade: feedback.and_then(|fb| fb.get("gradefordisplay")).and_then(|v| v.as_str()).map(String::from),
        feedback: comments_plugin,
        last_modified: submission.and_then(|s| s.get("timemodified")).and_then(|v| v.as_i64()),
        extensions,
    })
}

/// Save/submit an assignment.
pub async fn save_submission_api(
    client: &Client,
    session: &SessionInfo,
    assignment_id: u64,
    online_text: Option<&str>,
    file_id: Option<u64>,
) -> anyhow::Result<()> {
    let site_info = get_site_info(client, session).await?;
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    let mut plugins = Vec::new();
    if let Some(text) = online_text {
        plugins.push(serde_json::json!({
            "type": "onlinetext",
            "online_text": { "text": text, "format": 1, "itemid": 0 }
        }));
    }
    if let Some(fid) = file_id {
        plugins.push(serde_json::json!({
            "type": "file",
            "files_filemanager": fid
        }));
    }

    let args = moodle_args!(
        "assignmentid" => assignment_id,
        "userid" => site_info.userid,
        "plugins" => plugins
    );
    moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_assign_save_submission", &args).await?;
    Ok(())
}
