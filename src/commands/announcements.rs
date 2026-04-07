use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_site_info;
use crate::moodle::message::get_messages_api;
use crate::moodle::forum::get_discussion_posts_api;
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use super::ApiCtx;

pub async fn run(cmd: &crate::AnnouncementsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::AnnouncementsCommands::ListAll { level, unread_only, limit } => {
            let site_info = get_site_info(&ctx.client, &ctx.session).await?;
            let messages = get_messages_api(
                &ctx.client, &ctx.session,
                site_info.userid, None,
                if *unread_only { Some(false) } else { None },
                Some(*limit),
            ).await?;

            let mut items: Vec<serde_json::Value> = messages.iter().map(|m| {
                let unread = m.timecreated > 0; // Moodle core_message_get_messages with read=false only returns unread
                serde_json::json!({
                    "course_id": 0,
                    "course_name": "Notifications",
                    "id": m.id,
                    "subject": m.subject,
                    "author": format!("User {}", m.useridfrom),
                    "author_id": m.useridfrom,
                    "created_at": format_moodle_date(Some(m.timecreated)),
                    "unread": unread,
                })
            }).collect();

            // TODO: course-level filtering when messages include course context
            let _level = level;

            // Sort by created_at descending (already strings, lexicographic sort works for ISO dates)
            items.sort_by(|a, b| {
                let ta = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                let tb = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                tb.cmp(ta)
            });

            let shown: Vec<_> = items.into_iter().take(*limit as usize).collect();

            ctx.log.info(&format!("Showing {} announcements", shown.len()));
            format_and_output(&shown, ctx.output, None);
        }

        crate::AnnouncementsCommands::Read { announcement_id } => {
            let posts = get_discussion_posts_api(
                &ctx.client, &ctx.session, *announcement_id,
            ).await?;

            if posts.is_empty() {
                anyhow::bail!("Announcement not found: {}", announcement_id);
            }

            let first = &posts[0];
            let item = serde_json::json!({
                "id": announcement_id,
                "subject": first.subject,
                "author": first.author,
                "author_id": first.author_id,
                "created_at": format_moodle_date(Some(first.created)),
                "modified_at": format_moodle_date(Some(first.modified)),
                "message": first.message,
            });

            format_and_output(&[item], ctx.output, None);
        }
    }

    Ok(())
}
