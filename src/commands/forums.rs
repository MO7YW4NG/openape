use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::forum::{
    get_forums_api, get_forum_discussions_api, get_discussion_posts_api,
    add_discussion_api, add_discussion_post_api, delete_post_api,
};
use crate::output::format_and_output;
use crate::utils::{strip_html_tags, format_moodle_date};
use super::{ApiCtx, level_to_classification};

pub async fn run(cmd: &crate::ForumsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::ForumsCommands::List => {
            list_forums(&ctx, "inprogress").await?;
        }

        crate::ForumsCommands::ListAll { level } => {
            let classification = level_to_classification(*level);
            list_forums(&ctx, classification).await?;
        }

        crate::ForumsCommands::Discussions { forum_id } => {
            let discussions = get_forum_discussions_api(
                &ctx.client, &ctx.session, *forum_id, None, None, None, None,
            ).await?;

            let items: Vec<serde_json::Value> = discussions.iter().map(|d| serde_json::json!({
                "id": d.id,
                "name": d.name,
                "user_id": d.user_id,
                "time_modified": d.time_modified,
                "post_count": d.post_count,
                "unread": d.unread,
                "message": d.message.as_deref().map(strip_html_tags),
            })).collect();

            ctx.log.info(&format!("Forum {}: {} discussions", forum_id, items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::ForumsCommands::Posts { discussion_id } => {
            let posts = get_discussion_posts_api(
                &ctx.client, &ctx.session, *discussion_id,
            ).await?;

            let items: Vec<serde_json::Value> = posts.iter().map(|p| serde_json::json!({
                "id": p.id,
                "subject": p.subject,
                "author": p.author,
                "author_id": p.author_id,
                "created": format_moodle_date(Some(p.created)),
                "modified": format_moodle_date(Some(p.modified)),
                "message": p.message,
                "unread": p.unread,
            })).collect();

            ctx.log.info(&format!("Discussion {}: {} posts", discussion_id, items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::ForumsCommands::Post { forum_id, subject, message, .. } => {
            ctx.log.info(&format!("Posting to forum {}...", forum_id));
            match add_discussion_api(&ctx.client, &ctx.session, *forum_id, subject, message).await? {
                Some(discussion_id) => {
                    ctx.log.success("Discussion posted successfully!");
                    ctx.log.info(&format!("  Discussion ID: {}", discussion_id));
                    let result = serde_json::json!({
                        "action": "post",
                        "forum_id": *forum_id,
                        "discussion_id": discussion_id,
                        "success": true,
                    });
                    format_and_output(&[result], ctx.output, None);
                }
                None => {
                    anyhow::bail!("Post appeared to succeed but no discussion ID returned");
                }
            }
        }

        crate::ForumsCommands::Reply { post_id, subject, message, attachment_id, inline_attachment_id } => {
            ctx.log.info(&format!("Replying to post {}...", post_id));
            match add_discussion_post_api(
                &ctx.client, &ctx.session, *post_id, subject, message,
                *inline_attachment_id, *attachment_id,
            ).await? {
                Some(new_post_id) => {
                    ctx.log.success("Reply posted successfully!");
                    ctx.log.info(&format!("  Post ID: {}", new_post_id));
                    let result = serde_json::json!({
                        "action": "reply",
                        "post_id": *post_id,
                        "new_post_id": new_post_id,
                        "success": true,
                    });
                    format_and_output(&[result], ctx.output, None);
                }
                None => {
                    anyhow::bail!("Reply appeared to succeed but no post ID returned");
                }
            }
        }

        crate::ForumsCommands::Delete { post_id } => {
            let ok = delete_post_api(&ctx.client, &ctx.session, *post_id).await?;
            if ok {
                ctx.log.success(&format!("Post {} deleted successfully!", post_id));
                let result = serde_json::json!({
                    "action": "delete",
                    "post_id": *post_id,
                    "success": true,
                });
                format_and_output(&[result], ctx.output, None);
            } else {
                anyhow::bail!("Failed to delete post {}", post_id);
            }
        }
    }

    Ok(())
}

async fn list_forums(ctx: &ApiCtx, classification: &str) -> Result<()> {
    let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
    let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
    let forums = get_forums_api(&ctx.client, &ctx.session, &course_ids).await?;

    let course_map: std::collections::HashMap<u64, &str> = courses.iter()
        .map(|c| (c.id, c.fullname.as_str()))
        .collect();

    let items: Vec<serde_json::Value> = forums.iter().filter_map(|f| {
        let course_id = f.get("course")
            .or_else(|| f.get("courseid"))
            .and_then(|v| v.as_u64())?;
        let course_name = course_map.get(&course_id).copied().unwrap_or("Unknown");
        Some(serde_json::json!({
            "course_id": course_id,
            "course_name": course_name,
            "cmid": f.get("cmid"),
            "forum_id": f.get("id"),
            "name": f.get("name"),
            "intro": f.get("intro").and_then(|v| v.as_str()).map(strip_html_tags),
            "timemodified": f.get("timemodified"),
        }))
    }).collect();

    ctx.log.info(&format!(
        "Found {} courses, {} forums",
        courses.len(), items.len()
    ));
    format_and_output(&items, ctx.output, None);
    Ok(())
}
