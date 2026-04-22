use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{CourseGrade, GradeItem, SessionInfo};
use reqwest::Client;

/// Get course grades via WS API.
pub async fn get_course_grades_api(
    client: &Client,
    session: &SessionInfo,
    course_id: u64,
    user_id: u64,
) -> anyhow::Result<CourseGrade> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("courseid" => course_id, "userid" => user_id);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "gradereport_user_get_grade_items", &args).await?;

    let usergrades = data.get("usergrades").and_then(|u| u.as_array()).cloned().unwrap_or_default();
    let first = usergrades.first();

    let gradeitems = first.and_then(|f| f.get("gradeitems")).and_then(|g| g.as_array()).cloned().unwrap_or_default();

    let mut course_total_grade = None;

    let items: Vec<GradeItem> = gradeitems.iter().filter_map(|g| {
        let itemtype = g.get("itemtype").and_then(|v| v.as_str()).unwrap_or("");
        if itemtype == "course" {
            course_total_grade = g.get("graderaw").and_then(|v| v.as_f64()).map(|v| format!("{}", v as u64));
            return None;
        }

        let graderaw = g.get("graderaw");
        Some(GradeItem {
            name: g.get("itemname").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            grade: graderaw.and_then(|v| v.as_f64()).map(|v| format!("{}", v as u64)),
            percentage: g.get("percentageformatted").and_then(|v| v.as_str())
                .and_then(|s| s.trim_end_matches(" %").parse::<f64>().ok()),
            weight: g.get("weightraw").and_then(|v| v.as_f64()),
            feedback: g.get("feedback").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
            graded: graderaw.map_or(false, |v| !v.is_null()),
        })
    }).collect();

    Ok(CourseGrade {
        course_id,
        course_name: String::new(),
        grade: course_total_grade,
        items: Some(items),
    })
}
