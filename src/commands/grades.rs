use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::grade::get_course_grades_api;
use crate::output::format_and_output;
use super::ApiCtx;

pub async fn run(cmd: &crate::GradesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::GradesCommands::Summary => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;

            let mut summaries: Vec<serde_json::Value> = Vec::new();
            let mut ranked_count = 0u32;
            let mut rank_sum = 0u64;

            for course in &courses {
                match get_course_grades_api(&ctx.client, &ctx.session, course.id).await {
                    Ok(grade) => {
                        if let (Some(rank), Some(_total)) = (grade.rank, grade.total_users) {
                            ranked_count += 1;
                            rank_sum += rank as u64;
                        }
                        summaries.push(serde_json::json!({
                            "courseId": grade.course_id,
                            "courseName": grade.course_name,
                            "grade": grade.grade,
                            "gradeFormatted": grade.grade_formatted,
                            "rank": grade.rank,
                            "totalUsers": grade.total_users,
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

            let avg_rank = if ranked_count > 0 {
                format!("{:.1}", rank_sum as f64 / ranked_count as f64)
            } else {
                "N/A".to_string()
            };

            ctx.log.info(&format!(
                "Total: {} courses, {} graded, avg rank: {}",
                courses.len(), graded, avg_rank
            ));

            format_and_output(&summaries, ctx.output, None);
        }

        crate::GradesCommands::Course { course_id } => {
            let grade = get_course_grades_api(&ctx.client, &ctx.session, *course_id).await?;

            let items: Vec<serde_json::Value> = grade.items.as_deref().unwrap_or(&[]).iter()
                .map(|item| serde_json::json!({
                    "name": item.name,
                    "grade": item.grade,
                    "gradeFormatted": item.grade_formatted,
                    "range": item.range,
                    "percentage": item.percentage,
                    "weight": item.weight,
                    "feedback": item.feedback,
                    "graded": item.graded,
                }))
                .collect();

            let result = serde_json::json!({
                "courseId": grade.course_id,
                "courseName": grade.course_name,
                "grade": grade.grade,
                "gradeFormatted": grade.grade_formatted,
                "rank": grade.rank,
                "totalUsers": grade.total_users,
                "items": items,
            });

            format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}
