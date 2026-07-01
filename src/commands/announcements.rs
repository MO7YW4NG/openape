use super::ApiCtx;
use crate::moodle::message::get_messages_api;
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use crate::Cli;
use anyhow::Result;

pub async fn run(cmd: &crate::AnnouncementsCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::AnnouncementsCommands::ListAll { unread_only, limit } => {
            let messages = get_messages_api(
                &ctx.client,
                &ctx.session,
                ctx.session.user_id,
                None,
                if *unread_only { Some(false) } else { None },
                Some(*limit),
            )
            .await?;

            let mut items: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "course_id": 0,
                        "course_name": "Notifications",
                        "id": m.id,
                        "subject": m.subject,
                        "author": format!("User {}", m.useridfrom),
                        "author_id": m.useridfrom,
                        "created_at": format_moodle_date(Some(m.timecreated)),
                        "unread": !m.read,
                    })
                })
                .collect();

            // Sort by created_at descending (already strings, lexicographic sort works for ISO dates)
            items.sort_by(|a, b| {
                let ta = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                let tb = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                tb.cmp(ta)
            });

            let shown: Vec<_> = items.into_iter().take(*limit as usize).collect();

            ctx.log
                .info(&format!("Showing {} announcements", shown.len()));
            format_and_output(&shown, ctx.output, None);
        }

        crate::AnnouncementsCommands::Read { announcement_id } => {
            let messages = get_messages_api(
                &ctx.client,
                &ctx.session,
                ctx.session.user_id,
                None,
                None,
                None,
            )
            .await?;
            let message = messages
                .iter()
                .find(|message| message.id == *announcement_id)
                .ok_or_else(|| anyhow::anyhow!("Announcement not found: {}", announcement_id))?;
            let item = serde_json::json!({
                "id": announcement_id,
                "subject": message.subject,
                "author": format!("User {}", message.useridfrom),
                "author_id": message.useridfrom,
                "created_at": format_moodle_date(Some(message.timecreated)),
                "message": message.text,
            });

            format_and_output(&[item], ctx.output, None);
        }
    }

    Ok(())
}
