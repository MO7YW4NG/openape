use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::calendar::get_calendar_events_api;
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use std::fs;
use super::ApiCtx;

pub async fn run(cmd: &crate::CalendarCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::CalendarCommands::Events { upcoming, days, course } => {
            let now = chrono::Utc::now().timestamp();
            let end_time = now + (*days as i64 * 86400);

            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;

            let mut all_events = Vec::new();

            if let Some(course_id) = course {
                let events = get_calendar_events_api(
                    &ctx.client, &ctx.session,
                    Some(*course_id), Some(now), Some(end_time),
                ).await?;
                all_events.extend(events.into_iter().filter(|e| e.courseid == Some(*course_id)));
            } else {
                for c in &courses {
                    match get_calendar_events_api(
                        &ctx.client, &ctx.session,
                        Some(c.id), Some(now), Some(end_time),
                    ).await {
                        Ok(events) => all_events.extend(events),
                        Err(e) => ctx.log.warn(&format!("Failed to get events for course {}: {}", c.id, e)),
                    }
                }
            }

            all_events.sort_by_key(|e| e.timestart);

            let filtered: Vec<_> = if *upcoming {
                all_events.iter().filter(|e| e.timestart > now).collect()
            } else {
                all_events.iter().collect()
            };

            let items: Vec<serde_json::Value> = filtered.iter().map(|e| serde_json::json!({
                "id": e.id,
                "name": e.name,
                "description": e.description,
                "course_id": e.courseid,
                "event_type": e.eventtype,
                "start_time": format_moodle_date(Some(e.timestart)),
                "end_time": e.timeduration.map(|d| format_moodle_date(Some(e.timestart + d))),
                "location": e.location,
            })).collect();

            let upcoming_count = all_events.iter().filter(|e| e.timestart > now).count();
            ctx.log.info(&format!(
                "Total: {} events, {} upcoming",
                all_events.len(), upcoming_count
            ));

            format_and_output(&items, ctx.output, None);
        }

        crate::CalendarCommands::Export { output, days } => {
            let now = chrono::Utc::now().timestamp();
            let end_time = now + (*days as i64 * 86400);

            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "inprogress").await?;
            let mut all_events = Vec::new();

            for c in &courses {
                match get_calendar_events_api(
                    &ctx.client, &ctx.session,
                    Some(c.id), Some(now), Some(end_time),
                ).await {
                    Ok(events) => all_events.extend(events),
                    Err(e) => ctx.log.warn(&format!("Failed to get events for course {}: {}", c.id, e)),
                }
            }

            all_events.sort_by_key(|e| e.timestart);

            let mut by_type = serde_json::Map::new();
            for event in &all_events {
                let count = by_type.entry(event.eventtype.clone())
                    .or_insert(serde_json::json!(0));
                *count = serde_json::json!(count.as_u64().unwrap_or(0) + 1);
            }

            let export_data = serde_json::json!({
                "exported_at": chrono::Utc::now().to_rfc3339(),
                "time_range": {
                    "start": chrono::DateTime::from_timestamp(now, 0)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                    "end": chrono::DateTime::from_timestamp(end_time, 0)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                    "days": days,
                },
                "events": all_events.iter().map(|e| serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "description": e.description,
                    "course_id": e.courseid,
                    "event_type": e.eventtype,
                    "start_time": format_moodle_date(Some(e.timestart)),
                    "end_time": e.timeduration.map(|d| format_moodle_date(Some(e.timestart + d))),
                    "location": e.location,
                })).collect::<Vec<_>>(),
                "summary": {
                    "total_events": all_events.len(),
                    "by_type": serde_json::Value::Object(by_type),
                },
            });

            let json_str = serde_json::to_string_pretty(&export_data)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, &json_str)?;
            ctx.log.success(&format!("Exported {} events to {}", all_events.len(), output.display()));
        }
    }

    Ok(())
}
