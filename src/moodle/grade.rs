use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{CourseGrade, GradeItem, SessionInfo};
use reqwest::Client;

/// Get course grades via WS API.
pub async fn get_course_grades_api(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
) -> anyhow::Result<CourseGrade> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("courseid" => course_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "gradereport_user_get_grade_items", &args).await?;

    let usergrades = data.get("usergrades").and_then(|u| u.as_array()).cloned().unwrap_or_default();
    let first = usergrades.first();

    let items: Vec<GradeItem> = usergrades.iter().map(|g| {
        GradeItem {
            id: g.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: g.get("itemname").or_else(|| g.get("itemtype"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string(),
            grade: g.get("grade").and_then(|v| v.as_str()).map(String::from),
            grade_formatted: g.get("gradeformatted").and_then(|v| v.as_str()).map(String::from),
            range: if g.get("grade").is_some() {
                let min = g.get("grademin").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let max = g.get("grademax").and_then(|v| v.as_f64()).unwrap_or(100.0);
                Some(format!("{}-{}", min, max))
            } else { None },
            percentage: g.get("percentage").and_then(|v| v.as_f64()),
            weight: g.get("weight").and_then(|v| v.as_f64()),
            feedback: None,
            graded: g.get("grade").is_some(),
        }
    }).collect();

    Ok(CourseGrade {
        course_id,
        course_name: first.and_then(|f| f.get("coursefullname")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
        grade: first.and_then(|f| f.get("grade")).and_then(|v| v.as_str()).map(String::from),
        grade_formatted: first.and_then(|f| f.get("gradeformatted")).and_then(|v| v.as_str()).map(String::from),
        rank: first.and_then(|f| f.get("rank")).and_then(|v| v.as_u64()).map(|r| r as u32),
        total_users: first.and_then(|f| f.get("totalusers")).and_then(|v| v.as_u64()).map(|t| t as u32),
        items: Some(items),
    })
}
