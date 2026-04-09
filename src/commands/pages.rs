use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::page::get_pages_by_courses_api;
use crate::output::format_and_output;
use super::{ApiCtx, in_progress_all_to_classification, level_to_classification};

pub async fn run(cmd: &crate::PagesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::PagesCommands::List { course_id } => {
            let pages = get_pages_by_courses_api(&ctx.client, &ctx.session, &[*course_id]).await?;

            let items: Vec<serde_json::Value> = pages.iter().map(|p| serde_json::json!({
                "cmid": p.cmid,
                "name": p.name,
                "content": truncate_str(p.content.as_deref(), 150),
                "timemodified": p.timemodified,
            })).collect();

            ctx.log.info(&format!("Found {} pages", items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::PagesCommands::ListAll { level } => {
            let classification = in_progress_all_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let pages = get_pages_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            let course_map: std::collections::HashMap<u64, &str> = courses.iter()
                .map(|c| (c.id, c.fullname.as_str()))
                .collect();

            let items: Vec<serde_json::Value> = pages.iter().map(|p| serde_json::json!({
                "course_id": p.course_id,
                "course_name": course_map.get(&p.course_id).copied().unwrap_or("Unknown"),
                "cmid": p.cmid,
                "name": p.name,
                "content": truncate_str(p.content.as_deref(), 150),
                "timemodified": p.timemodified,
            })).collect();

            ctx.log.info(&format!("Found {} pages across {} courses", items.len(), courses.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::PagesCommands::Show { cmid } => {
            // Get the course content for the page module to find pages
            let classification = level_to_classification(crate::CourseLevel::All);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let pages = get_pages_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            let page = pages.iter().find(|p| p.cmid == cmid.to_string());
            match page {
                Some(p) => {
                    let result = serde_json::json!({
                        "cmid": *cmid,
                        "name": p.name,
                        "content": p.content.as_deref().unwrap_or(""),
                        "timemodified": p.timemodified,
                    });
                    format_and_output(&[result], ctx.output, None);
                }
                None => {
                    anyhow::bail!("Page with cmid {} not found", cmid);
                }
            }
        }
    }

    Ok(())
}

fn truncate_str(s: Option<&str>, max: usize) -> Option<String> {
    s.map(|t| {
        let trimmed: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.chars().count() > max {
            let truncated: String = trimmed.chars().take(max).collect();
            format!("{}...", truncated.trim_end())
        } else {
            trimmed
        }
    })
}
