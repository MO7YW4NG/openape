use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::quiz::{
    get_quizzes_by_courses_api, start_quiz_attempt_api,
    get_all_quiz_attempt_data_api, process_quiz_attempt_api,
};
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use super::{ApiCtx, level_to_classification};

pub async fn run(cmd: &crate::QuizzesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli.config.as_ref(), cli.output, cli.verbose, cli.silent)?;

    match cmd {
        crate::QuizzesCommands::List { course_id, all } => {
            let quizzes = get_quizzes_by_courses_api(&ctx.client, &ctx.session, &[*course_id]).await?;

            let filtered: Vec<_> = if *all {
                quizzes.iter().collect()
            } else {
                quizzes.iter().filter(|q| !q.is_complete).collect()
            };

            let items: Vec<serde_json::Value> = filtered.iter().map(|q| serde_json::json!({
                "quizid": q.quizid,
                "name": q.name,
                "url": q.url,
                "is_complete": q.is_complete,
                "attempts_used": q.attempts_used,
                "max_attempts": q.max_attempts,
                "time_open": format_moodle_date(q.time_open),
                "time_close": format_moodle_date(q.time_close),
            })).collect();

            ctx.log.info(&format!("Found {} quizzes", items.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::QuizzesCommands::ListAll { level, all } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;
            let course_ids: Vec<u64> = courses.iter().map(|c| c.id).collect();
            let quizzes = get_quizzes_by_courses_api(&ctx.client, &ctx.session, &course_ids).await?;

            let course_map: std::collections::HashMap<Option<u64>, &str> = courses.iter()
                .map(|c| (Some(c.id), c.fullname.as_str()))
                .collect();

            let filtered: Vec<_> = if *all {
                quizzes.iter().collect()
            } else {
                quizzes.iter().filter(|q| !q.is_complete).collect()
            };

            let items: Vec<serde_json::Value> = filtered.iter().map(|q| serde_json::json!({
                "quizid": q.quizid,
                "course_name": course_map.get(&q.course_id).copied().unwrap_or("Unknown"),
                "name": q.name,
                "url": q.url,
                "is_complete": q.is_complete,
                "attempts_used": q.attempts_used,
                "max_attempts": q.max_attempts,
                "time_close": format_moodle_date(q.time_close),
            })).collect();

            ctx.log.info(&format!("Found {} quizzes across {} courses", items.len(), courses.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::QuizzesCommands::Start { quiz_id } => {
            let result = start_quiz_attempt_api(&ctx.client, &ctx.session, *quiz_id, false).await?;
            let attempt = &result.attempt;
            ctx.log.success(&format!("Quiz attempt started! Attempt ID: {}", attempt.attemptid));
            if let Some(msgs) = &result.messages {
                for msg in msgs {
                    ctx.log.info(&format!("  Note: {}", msg));
                }
            }
            let item = serde_json::json!({
                "attemptId": attempt.attemptid,
                "quizId": attempt.quizid,
                "state": attempt.state,
                "startPage": result.page.unwrap_or(0),
            });
            format_and_output(&[item], ctx.output, None);
        }

        crate::QuizzesCommands::Info { attempt_id, page } => {
            let data = if *page == -1 {
                get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, *attempt_id).await?
            } else {
                crate::moodle::quiz::get_quiz_attempt_data_api(
                    &ctx.client, &ctx.session, *attempt_id, *page,
                ).await?
            };

            let mut questions: Vec<serde_json::Value> = data.questions.values().map(|q| serde_json::json!({
                "slot": q.slot,
                "type": q.qtype,
                "maxmark": q.maxmark,
                "status": q.status,
                "questionnumber": q.questionnumber,
                "html": q.html.as_deref().map(|h| {
                    // Strip HTML for cleaner output
                    let re = regex::Regex::new(r"<[^>]+>").unwrap_or_else(|_| regex::Regex::new(r"x").unwrap());
                    re.replace_all(h, "").into_owned()
                }),
            })).collect();

            questions.sort_by_key(|q| q.get("slot").and_then(|v| v.as_u64()).unwrap_or(0));

            ctx.log.info(&format!("Attempt {} has {} questions", attempt_id, questions.len()));
            format_and_output(&questions, ctx.output, None);
        }

        crate::QuizzesCommands::Save { attempt_id, answers, submit } => {
            // Get attempt data first for unique_id and sequence_checks
            let data = get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, *attempt_id).await?;
            let unique_id = data.attempt.uniqueid
                .ok_or_else(|| anyhow::anyhow!("Could not get attempt unique ID"))?;

            // Parse answers: "slot:answer,slot:answer" format
            let parsed_answers: Vec<(u32, String)> = answers.split(';')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, ':');
                    let slot: u32 = parts.next()?.trim().parse().ok()?;
                    let answer = parts.next()?.trim().to_string();
                    Some((slot, answer))
                })
                .collect();

            let sequence_checks: std::collections::HashMap<u32, u64> = data.questions.iter()
                .filter_map(|(&slot, q)| q.sequencecheck.map(|sc| (slot, sc)))
                .collect();

            let state = process_quiz_attempt_api(
                &ctx.client, &ctx.session,
                *attempt_id, unique_id,
                &parsed_answers, &sequence_checks,
                *submit,
            ).await?;

            if *submit {
                ctx.log.success(&format!("Quiz submitted! State: {}", state));
            } else {
                ctx.log.success(&format!("Answers saved. State: {}", state));
            }
        }
    }

    Ok(())
}
