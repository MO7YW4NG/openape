use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::grade::get_course_grades_api;
use crate::output::format_and_output;
use super::ApiCtx;

pub async fn run(cmd: &crate::GradesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::GradesCommands::Summary => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;

            let mut summaries: Vec<serde_json::Value> = Vec::new();

            for course in &courses {
                match get_course_grades_api(&ctx.client, &ctx.session, course.id, ctx.session.user_id).await {
                    Ok(mut grade) => {
                        grade.course_name = course.fullname.clone();
                        summaries.push(serde_json::json!({
                            "courseId": grade.course_id,
                            "courseName": grade.course_name,
                            "grade": grade.grade,
                        }));
                    }
                    Err(e) => {
                        ctx.log.warn(&format!("Failed to get grades for course {}: {}", course.id, e));
                    }
                }
            }

            let graded = summaries.iter().filter(|g| {
                g.get("grade").map(|v| !v.is_null() && v.as_str() != Some("-")).unwrap_or(false)
            }).count();

            ctx.log.info(&format!(
                "Total: {} courses, {} graded",
                courses.len(), graded,
            ));

            format_and_output(&summaries, ctx.output, None);
        }

        crate::GradesCommands::Course { course_id } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "all").await?;
            let course_name = courses.iter()
                .find(|c| c.id == *course_id)
                .map(|c| c.fullname.as_str())
                .unwrap_or("");

            let mut grade = get_course_grades_api(&ctx.client, &ctx.session, *course_id, ctx.session.user_id).await?;
            grade.course_name = course_name.to_string();

            let mut rows: Vec<serde_json::Value> = Vec::new();
            rows.push(serde_json::json!({
                "courseId": grade.course_id,
                "courseName": &grade.course_name,
                "grade": grade.grade,
            }));

            for item in grade.items.as_deref().unwrap_or(&[]) {
                rows.push(serde_json::json!({
                    "name": item.name,
                    "grade": item.grade,
                    "percentage": item.percentage,
                    "weight": item.weight,
                    "feedback": item.feedback,
                    "graded": item.graded,
                }));
            }

            ctx.log.info(&format!("Found {} grade items", rows.len() - 1));
            format_and_output(&rows, ctx.output, None);
        }
    }

    Ok(())
}
