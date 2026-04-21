use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::moodle::quiz::{
    get_quizzes_by_courses_api, start_quiz_attempt_api,
    get_all_quiz_attempt_data_api, process_quiz_attempt_api,
};
use crate::output::format_and_output;
use crate::utils::{format_moodle_date};
use super::{ApiCtx, in_progress_all_to_classification};
use std::collections::HashSet;

pub async fn run(cmd: &crate::QuizzesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

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
                "cmid": q.cmid,
                "name": q.name,
                "intro": q.intro,
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
            let classification = in_progress_all_to_classification(*level);
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
                "cmid": q.cmid,
                "course_name": course_map.get(&q.course_id).copied().unwrap_or("Unknown"),
                "name": q.name,
                "intro": q.intro,
                "url": q.url,
                "is_complete": q.is_complete,
                "attempts_used": q.attempts_used,
                "max_attempts": q.max_attempts,
                "time_close": format_moodle_date(q.time_close),
            })).collect();

            ctx.log.info(&format!("Found {} quizzes across {} courses", items.len(), courses.len()));
            format_and_output(&items, ctx.output, None);
        }

        crate::QuizzesCommands::Start { quiz_id, cmid } => {
            let result = start_quiz_attempt_api(&ctx.client, &ctx.session, *quiz_id, false, *cmid).await?;
            let attempt = &result.attempt;
            ctx.log.success(&format!("Quiz attempt started! Attempt ID: {}", attempt.attemptid));
            if let Some(msgs) = &result.messages {
                for msg in msgs {
                    ctx.log.info(&format!("  Note: {}", msg));
                }
            }

            // Auto-fetch questions like the TS version
            match get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, attempt.attemptid, None).await {
                Ok(data) => {
                    let questions: Vec<serde_json::Value> = data.questions.values()
                        .map(|q| format_question_json(q))
                        .collect();
                    let total = questions.len();

                    let mut items: Vec<serde_json::Value> = vec![serde_json::json!({
                        "_type": "attempt",
                        "attemptId": attempt.attemptid,
                        "quizId": attempt.quizid,
                        "state": attempt.state,
                        "timeStart": format_moodle_date(Some(attempt.timestart)),
                        "timeFinish": format_moodle_date(attempt.timefinish),
                        "isPreview": attempt.preview,
                        "totalQuestions": total,
                    })];
                    items.extend(questions);
                    format_and_output(&items, ctx.output, None);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("找不到資料記錄") || msg.contains("record not found") {
                        ctx.log.warn("Attempt data not available yet — use `openape quizzes info <attempt-id>` to fetch questions later.");
                        let result = serde_json::json!({
                            "_type": "attempt",
                            "attemptId": attempt.attemptid,
                            "quizId": attempt.quizid,
                            "state": attempt.state,
                            "totalQuestions": 0,
                        });
                        format_and_output(&[result], ctx.output, None);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        crate::QuizzesCommands::Info { attempt_id, page, cmid } => {
            let data = if *page == -1 {
                get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, *attempt_id, *cmid).await?
            } else {
                crate::moodle::quiz::get_quiz_attempt_data_api(
                    &ctx.client, &ctx.session, *attempt_id, *page, *cmid,
                ).await?
            };

            let mut questions: Vec<serde_json::Value> = data.questions.values()
                .map(|q| format_question_json(q))
                .collect();
            questions.sort_by_key(|q| q.get("slot").and_then(|v| v.as_u64()).unwrap_or(0));

            // First row: attempt metadata, then each question as a separate NDJSON row
            let total = questions.len();
            let mut items: Vec<serde_json::Value> = vec![serde_json::json!({
                "_type": "attempt",
                "attemptId": data.attempt.attemptid,
                "quizId": data.attempt.quizid,
                "state": data.attempt.state,
                "totalQuestions": total,
            })];
            items.extend(questions);

            ctx.log.info(&format!("Attempt {} has {} questions", attempt_id, total));
            format_and_output(&items, ctx.output, None);
        }

        crate::QuizzesCommands::Save { attempt_id, answers, cmid } => {
            // Get attempt data first for unique_id and sequence_checks
            let data = get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, *attempt_id, *cmid).await?;
            let unique_id = data.attempt.uniqueid
                .ok_or_else(|| anyhow::anyhow!("Could not get attempt unique ID"))?;

            // Parse answers from either JSON or escaped delimiter format.
            let parsed_answers = parse_answers_input(answers)?;

            let sequence_checks: std::collections::HashMap<u32, u64> = data.questions.iter()
                .filter_map(|(&slot, q)| q.sequencecheck.map(|sc| (slot, sc)))
                .collect();

            // Detect checkbox-style multichoice questions from HTML so only those use choiceN submission.
            let checkbox_slots: HashSet<u32> = data.questions.iter()
                .filter_map(|(&slot, q)| {
                    let html = q.html.as_deref().unwrap_or("");
                    if html.contains("type=\"checkbox\"") || html.contains("type='checkbox'") {
                        Some(slot)
                    } else {
                        None
                    }
                })
                .collect();

            let state = process_quiz_attempt_api(
                &ctx.client, &ctx.session,
                *attempt_id, unique_id,
                &parsed_answers, &sequence_checks,
                &checkbox_slots,
                false, *cmid,
            ).await?;

            ctx.log.success(&format!("Answers saved. State: {}", state));
            let result = serde_json::json!({
                "action": "save",
                "attempt_id": *attempt_id,
                "submitted": false,
                "state": state,
            });
            format_and_output(&[result], ctx.output, None);
        }

        crate::QuizzesCommands::Submit { attempt_id, cmid } => {
            // Submit the attempt as-is, reusing answers already saved on Moodle.
            let data = get_all_quiz_attempt_data_api(&ctx.client, &ctx.session, *attempt_id, *cmid).await?;
            let unique_id = data.attempt.uniqueid
                .ok_or_else(|| anyhow::anyhow!("Could not get attempt unique ID"))?;

            let sequence_checks: std::collections::HashMap<u32, u64> = data.questions.iter()
                .filter_map(|(&slot, q)| q.sequencecheck.map(|sc| (slot, sc)))
                .collect();

            let checkbox_slots: HashSet<u32> = data.questions.iter()
                .filter_map(|(&slot, q)| {
                    let html = q.html.as_deref().unwrap_or("");
                    if html.contains("type=\"checkbox\"") || html.contains("type='checkbox'") {
                        Some(slot)
                    } else {
                        None
                    }
                })
                .collect();

            let parsed_answers: Vec<(u32, String)> = Vec::new();
            let state = process_quiz_attempt_api(
                &ctx.client, &ctx.session,
                *attempt_id, unique_id,
                &parsed_answers, &sequence_checks,
                &checkbox_slots,
                true, *cmid,
            ).await?;

            ctx.log.success(&format!("Quiz submitted! State: {}", state));
            let result = serde_json::json!({
                "action": "submit",
                "attempt_id": *attempt_id,
                "submitted": true,
                "state": state,
            });
            format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}

/// Format a QuizQuestion into a JSON value with parsed fields.
fn format_question_json(q: &crate::moodle::types::QuizQuestion) -> serde_json::Value {
    serde_json::json!({
        "slot": q.slot,
        "type": q.qtype,
        "status": q.status,
        "stateclass": q.stateclass,
        "savedAnswer": q.saved_answer,
        "question": q.question_text,
        "options": q.options,
    })
}


/// Convert single-letter choice answers (a-z, A-Z) to 0-based Moodle indices.
/// Passes through numeric strings, multi-char strings, and comma-separated lists unchanged.
fn maybe_convert_choice_index(s: &str) -> String {
    fn convert_one(s: &str) -> &str {
        match s {
            "a" | "A" => "0",
            "b" | "B" => "1",
            "c" | "C" => "2",
            "d" | "D" => "3",
            "e" | "E" => "4",
            "f" | "F" => "5",
            "g" | "G" => "6",
            "h" | "H" => "7",
            "i" | "I" => "8",
            "j" | "J" => "9",
            _ => return s,
        }
    }

    if s.len() == 1 {
        convert_one(s).to_string()
    } else if s.contains(',') {
        s.split(',').map(|p| convert_one(p.trim())).collect::<Vec<_>>().join(",")
    } else {
        s.to_string()
    }
}

fn parse_answers_input(raw: &str) -> anyhow::Result<Vec<(u32, String)>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Answers input is empty");
    }

    if trimmed.starts_with('[') {
        if let Ok(parsed) = parse_answers_json(trimmed) {
            return Ok(parsed);
        }
        if let Ok(parsed) = parse_answers_relaxed_object_list(trimmed) {
            return Ok(parsed);
        }
        anyhow::bail!(
            "Invalid answers JSON-like input. Expected JSON array of slot/answer objects"
        );
    }

    parse_answers_delimited(trimmed)
}

fn parse_answers_json(raw: &str) -> anyhow::Result<Vec<(u32, String)>> {
    #[derive(serde::Deserialize)]
    struct JsonAnswer {
        slot: u32,
        answer: serde_json::Value,
    }

    let parsed: Vec<JsonAnswer> = serde_json::from_str(raw)
        .map_err(|e| anyhow::anyhow!(
            "Invalid answers JSON. Expected an array of slot/answer objects, error: {}",
            e
        ))?;

    if parsed.is_empty() {
        anyhow::bail!("Answers JSON is empty");
    }

    let mut out = Vec::with_capacity(parsed.len());
    for item in parsed {
        if item.slot == 0 {
            anyhow::bail!("Invalid slot 0 in answers JSON");
        }
        let answer = normalize_json_answer(item.answer)?;
        out.push((item.slot, answer));
    }

    Ok(out)
}

fn normalize_json_answer(value: serde_json::Value) -> anyhow::Result<String> {
    match value {
        serde_json::Value::String(s) => Ok(maybe_convert_choice_index(&s)),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Array(arr) => {
            let mut parts = Vec::with_capacity(arr.len());
            for item in arr {
                match item {
                    serde_json::Value::String(s) => parts.push(s),
                    serde_json::Value::Number(n) => parts.push(n.to_string()),
                    serde_json::Value::Bool(b) => parts.push(b.to_string()),
                    serde_json::Value::Null => parts.push(String::new()),
                    _ => anyhow::bail!("Unsupported JSON answer array element type"),
                }
            }
            Ok(parts.join(","))
        }
        serde_json::Value::Object(_) => anyhow::bail!("Unsupported JSON answer object type"),
    }
}

fn parse_answers_relaxed_object_list(raw: &str) -> anyhow::Result<Vec<(u32, String)>> {
    // Supports PowerShell-native argument flattening like:
    // [{slot:1,answer:0},{slot:2,answer:"0,2"}]
    let s = raw.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        anyhow::bail!("Not a list-like input");
    }

    let object_re = regex::Regex::new(r"\{([^{}]+)\}").unwrap();
    let slot_re = regex::Regex::new(r"(?i)(?:^|,)\s*slot\s*:\s*([^,\s]+)").unwrap();
    let answer_re = regex::Regex::new(r#"(?i)(?:^|,)\s*answer\s*:\s*(\"[^\"]*\"|[^,}]+)"#).unwrap();

    let mut out = Vec::new();
    for caps in object_re.captures_iter(s) {
        let obj = caps.get(1).map(|m| m.as_str()).unwrap_or("");

        let slot_text = slot_re
            .captures(obj)
            .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .ok_or_else(|| anyhow::anyhow!("Missing slot in '{}'", obj))?;
        let slot: u32 = slot_text
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid slot '{}'", slot_text))?;
        if slot == 0 {
            anyhow::bail!("Invalid slot 0 in '{}'", obj);
        }

        let mut answer = answer_re
            .captures(obj)
            .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .ok_or_else(|| anyhow::anyhow!("Missing answer in '{}'", obj))?;

        if answer.starts_with('"') && answer.ends_with('"') && answer.len() >= 2 {
            answer = answer[1..answer.len() - 1].to_string();
        }

        out.push((slot, answer));
    }

    if out.is_empty() {
        anyhow::bail!("No objects found in list-like answers input");
    }

    Ok(out)
}

fn parse_answers_delimited(raw: &str) -> anyhow::Result<Vec<(u32, String)>> {
    let segments = split_unescaped(raw, ';')?;
    let mut out = Vec::new();

    for (idx, segment) in segments.into_iter().enumerate() {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }

        let (slot_part, answer_part) = split_once_unescaped(seg, ':')?
            .ok_or_else(|| anyhow::anyhow!(
                "Invalid answer segment #{}: '{}' (expected slot:answer)",
                idx + 1,
                seg
            ))?;

        let slot: u32 = slot_part.trim().parse().map_err(|_| {
            anyhow::anyhow!("Invalid slot in segment #{}: '{}'", idx + 1, slot_part)
        })?;

        if slot == 0 {
            anyhow::bail!("Invalid slot 0 in segment #{}", idx + 1);
        }

        let answer = maybe_convert_choice_index(&unescape_value(answer_part.trim())?);
        out.push((slot, answer));
    }

    if out.is_empty() {
        anyhow::bail!("No valid answers parsed. Format example: 1:0;2:my\\:text;3:a\\;b");
    }

    Ok(out)
}

fn split_unescaped(input: &str, delim: char) -> anyhow::Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == delim {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }

    if escaped {
        anyhow::bail!("Trailing escape character in answers input");
    }

    parts.push(current);
    Ok(parts)
}

fn split_once_unescaped(input: &str, delim: char) -> anyhow::Result<Option<(String, String)>> {
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == delim {
            let left = input[..idx].to_string();
            let right = input[idx + ch.len_utf8()..].to_string();
            return Ok(Some((left, right)));
        }
    }

    if escaped {
        anyhow::bail!("Trailing escape character in segment '{}'", input);
    }

    Ok(None)
}

fn unescape_value(input: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            match ch {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                ';' => out.push(';'),
                ':' => out.push(':'),
                other => out.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }

    if escaped {
        anyhow::bail!("Trailing escape character in answer value");
    }

    Ok(out)
}
