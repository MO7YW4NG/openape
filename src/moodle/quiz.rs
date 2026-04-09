use super::client::moodle_api_call;
use crate::moodle_args;
use super::types::{QuizAttempt, QuizAttemptData, QuizModule, QuizQuestion, QuizStartResult, SessionInfo};
use crate::utils::{parse_question_html, parse_saved_answer};
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Get quizzes by course IDs via WS API.
pub async fn get_quizzes_by_courses_api(
    client: &Client,
    session: &SessionInfo,
    course_ids: &[u64],
) -> anyhow::Result<Vec<QuizModule>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    if course_ids.is_empty() {
        return Ok(Vec::new());
    }

    let course_ids_json: Vec<Value> = course_ids.iter().map(|id| serde_json::json!(*id)).collect();
    let args = moodle_args!("courseids" => course_ids_json);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_quiz_get_quizzes_by_courses", &args).await?;

    let quizzes = data.get("quizzes").and_then(|q| q.as_array()).cloned().unwrap_or_default();

    // Get attempt info for each quiz
    let quiz_ids: Vec<u64> = quizzes.iter()
        .filter_map(|q| q.get("id").and_then(|v| v.as_u64()))
        .collect();
    let attempt_info = get_user_quiz_attempt_info(client, session, &quiz_ids).await?;

    Ok(quizzes.into_iter().map(|q| {
        let id = q.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let info = attempt_info.get(&id);
        QuizModule {
            quizid: id.to_string(),
            name: q.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            url: q.get("viewurl").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            is_complete: info.map(|i| i.0).unwrap_or(false),
            attempts_used: info.map(|i| i.1).unwrap_or(0),
            max_attempts: q.get("attempts").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            time_open: q.get("timeopen").and_then(|v| v.as_i64()),
            time_close: q.get("timeclose").and_then(|v| v.as_i64()),
            course_id: q.get("course").and_then(|v| v.as_u64()),
        }
    }).collect())
}

/// Get user quiz attempt info (parallel per quiz).
async fn get_user_quiz_attempt_info(
    client: &Client,
    session: &SessionInfo,
    quiz_ids: &[u64],
) -> anyhow::Result<HashMap<u64, (bool, u32)>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let mut info = HashMap::new();

    let mut handles = Vec::new();
    for &quiz_id in quiz_ids {
        let args = moodle_args!("quizid" => quiz_id);
        let c = client.clone();
        let base = session.moodle_base_url.clone();
        let tok = ws_token.clone();
        handles.push(tokio::spawn(async move {
            moodle_api_call(&c, &base, &tok, "mod_quiz_get_user_attempts", &args).await
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let quiz_id = quiz_ids[i];
        let mut used = 0u32;
        let mut finished = false;
        if let Ok(Ok(data)) = handle.await {
            if let Some(attempts) = data.get("attempts").and_then(|a| a.as_array()) {
                for a in attempts {
                    used += 1;
                    if a.get("state").and_then(|v| v.as_str()) == Some("finished") {
                        finished = true;
                    }
                }
            }
        }
        info.insert(quiz_id, (finished, used));
    }

    Ok(info)
}

/// Start a new quiz attempt.
pub async fn start_quiz_attempt_api(
    client: &Client,
    session: &SessionInfo,
    quiz_id: u64,
    force_new: bool,
) -> anyhow::Result<QuizStartResult> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!(
        "quizid" => quiz_id,
        "forcenew" => if force_new { 1 } else { 0 }
    );
    let data = match moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_quiz_start_attempt", &args).await {
        Ok(d) => d,
        Err(start_err) => {
            if let Some(existing) = get_latest_inprogress_attempt(client, session, quiz_id).await? {
                return Ok(QuizStartResult {
                    attempt: existing,
                    messages: Some(vec![
                        "Found existing in-progress attempt; reused it instead of creating a new one.".to_string(),
                    ]),
                });
            }
            return Err(start_err.into());
        }
    };

    let attempt = if let Some(a) = data.get("attempt") {
        a
    } else if let Some(existing) = get_latest_inprogress_attempt(client, session, quiz_id).await? {
        return Ok(QuizStartResult {
            attempt: existing,
            messages: Some(vec![
                "Start response did not include attempt payload; reused existing in-progress attempt.".to_string(),
            ]),
        });
    } else {
        anyhow::bail!("No attempt data in start response");
    };

    let parsed_attempt = parse_attempt(attempt);

    Ok(QuizStartResult {
        attempt: parsed_attempt,
        messages: data.get("messages").and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
    })
}

async fn get_latest_inprogress_attempt(
    client: &Client,
    session: &SessionInfo,
    quiz_id: u64,
) -> anyhow::Result<Option<QuizAttempt>> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!(
        "quizid" => quiz_id,
        "status" => "all",
    );
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_quiz_get_user_attempts", &args).await?;

    let attempts = data.get("attempts").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let latest = attempts.into_iter()
        .filter(|a| a.get("state").and_then(|v| v.as_str()) == Some("inprogress"))
        .max_by_key(|a| a.get("attempt").and_then(|v| v.as_u64()).unwrap_or(0));

    Ok(latest.as_ref().map(parse_attempt))
}

fn parse_attempt(attempt: &Value) -> QuizAttempt {
    let attempt_id = attempt.get("id").or_else(|| attempt.get("attempt"))
        .and_then(|v| v.as_u64()).unwrap_or(0);

    QuizAttempt {
        attempt: attempt_id,
        attemptid: attempt_id,
        quizid: attempt.get("quizid").or_else(|| attempt.get("quiz"))
            .and_then(|v| v.as_u64()).unwrap_or(0),
        userid: attempt.get("userid").and_then(|v| v.as_u64()).unwrap_or(0),
        attemptnumber: attempt.get("attemptnumber")
            .or_else(|| attempt.get("attempt"))
            .and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        state: attempt.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        timestart: attempt.get("timestart").and_then(|v| v.as_i64()).unwrap_or(0),
        timefinish: attempt.get("timefinish")
            .and_then(|v| v.as_i64())
            .and_then(|v| if v > 0 { Some(v) } else { None }),
        uniqueid: attempt.get("uniqueid").and_then(|v| v.as_u64()),
        preview: attempt.get("preview").and_then(|v| v.as_i64()) == Some(1),
    }
}

/// Get quiz attempt data for a specific page.
pub async fn get_quiz_attempt_data_api(
    client: &Client,
    session: &SessionInfo,
    attempt_id: u64,
    page: i32,
) -> anyhow::Result<QuizAttemptData> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;
    let args = moodle_args!("attemptid" => attempt_id, "page" => page);
    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_quiz_get_attempt_data", &args).await?;

    let attempt = data.get("attempt").ok_or_else(|| anyhow::anyhow!("Invalid attempt data"))?;
    let a_id = attempt.get("id").or_else(|| attempt.get("attempt"))
        .and_then(|v| v.as_u64()).unwrap_or(0);

    let mut questions = HashMap::new();
    if let Some(qs_raw) = data.get("questions") {
        let question_entries: Vec<(String, &Value)> = if let Some(arr) = qs_raw.as_array() {
            // Moodle returns questions as an array — use "slot" field as key
            arr.iter().filter_map(|q| {
                let slot = q.get("slot").and_then(|v| v.as_u64())?;
                Some((slot.to_string(), q))
            }).collect()
        } else if let Some(obj) = qs_raw.as_object() {
            obj.iter().map(|(k, v)| (k.clone(), v)).collect()
        } else {
            Vec::new()
        };

        for (slot, question) in question_entries {
            if let Ok(slot_num) = slot.parse::<u32>() {
                let html_raw = question.get("html").and_then(|v| v.as_str()).unwrap_or("");
                let (qtext, opts) = parse_question_html(html_raw);
                let saved = parse_saved_answer(html_raw);

                questions.insert(slot_num, QuizQuestion {
                    slot: question.get("slot").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    qtype: question.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    id: question.get("id").and_then(|v| v.as_u64()),
                    maxmark: question.get("maxmark").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    page: question.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    quizid: question.get("quizid").and_then(|v| v.as_u64()).unwrap_or(0),
                    html: Some(html_raw.to_string()),
                    status: question.get("status").and_then(|v| v.as_str()).map(String::from),
                    stateclass: question.get("stateclass").and_then(|v| v.as_str()).map(String::from),
                    sequencecheck: question.get("sequencecheck").and_then(|v| v.as_u64()),
                    questionnumber: question.get("questionnumber").and_then(|v| v.as_str()).map(String::from),
                    saved_answer: saved,
                    question_text: if qtext.is_empty() { None } else { Some(qtext) },
                    options: opts,
                });
            }
        }
    }

    Ok(QuizAttemptData {
        attempt: QuizAttempt {
            attempt: a_id,
            attemptid: a_id,
            uniqueid: attempt.get("uniqueid").and_then(|v| v.as_u64()),
            quizid: attempt.get("quizid").or_else(|| attempt.get("quiz"))
                .and_then(|v| v.as_u64()).unwrap_or(0),
            userid: attempt.get("userid").and_then(|v| v.as_u64()).unwrap_or(0),
            attemptnumber: attempt.get("attemptnumber").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            state: attempt.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            timestart: attempt.get("timestart").and_then(|v| v.as_i64()).unwrap_or(0),
            timefinish: attempt.get("timefinish").and_then(|v| v.as_i64()),
            preview: false,
        },
        questions,
        nextpage: data.get("nextpage").and_then(|v| v.as_i64()).map(|p| p as i32),
    })
}

/// Get all quiz attempt data (paginates through all pages).
pub async fn get_all_quiz_attempt_data_api(
    client: &Client,
    session: &SessionInfo,
    attempt_id: u64,
) -> anyhow::Result<QuizAttemptData> {
    let first = get_quiz_attempt_data_api(client, session, attempt_id, 0).await?;

    let mut all_questions = first.questions.clone();
    let mut next = first.nextpage;

    while let Some(page) = next {
        if page < 0 { break; }
        let page_data = get_quiz_attempt_data_api(client, session, attempt_id, page).await?;
        all_questions.extend(page_data.questions.clone());
        next = page_data.nextpage;
    }

    Ok(QuizAttemptData {
        attempt: first.attempt,
        questions: all_questions,
        nextpage: None,
    })
}

/// Process (save/finish) a quiz attempt.
pub async fn process_quiz_attempt_api(
    client: &Client,
    session: &SessionInfo,
    attempt_id: u64,
    unique_id: u64,
    answers: &[(u32, String)], // (slot, answer)
    sequence_checks: &HashMap<u32, u64>,
    checkbox_slots: &HashSet<u32>,
    finish: bool,
) -> anyhow::Result<String> {
    let ws_token = session.ws_token.as_ref().ok_or_else(|| anyhow::anyhow!("WS token required"))?;

    let mut args = HashMap::new();
    args.insert("attemptid".to_string(), serde_json::json!(attempt_id));
    args.insert("finishattempt".to_string(), serde_json::json!(if finish { 1 } else { 0 }));

    let numeric_csv_re = regex::Regex::new(r"^\d+(,\d+)*$").unwrap();

    let mut i = 0usize;
    for &(slot, ref answer) in answers {
        // Sequence check
        if let Some(&seq) = sequence_checks.get(&slot) {
            args.insert(format!("data[{}][name]", i), serde_json::json!(format!("q{}:{}_:sequencecheck", unique_id, slot)));
            args.insert(format!("data[{}][value]", i), serde_json::json!(seq));
            i += 1;
        }

        // Detect answer format
        if checkbox_slots.contains(&slot) && numeric_csv_re.is_match(answer) && answer.contains(',') {
            // Multichoices
            for choice in answer.split(',') {
                args.insert(format!("data[{}][name]", i), serde_json::json!(format!("q{}:{}_choice{}", unique_id, slot, choice)));
                args.insert(format!("data[{}][value]", i), serde_json::json!("1"));
                i += 1;
            }
        } else {
            args.insert(format!("data[{}][name]", i), serde_json::json!(format!("q{}:{}_answer", unique_id, slot)));
            args.insert(format!("data[{}][value]", i), serde_json::json!(answer));
            i += 1;
        }
    }

    let data = moodle_api_call(client, &session.moodle_base_url, ws_token,
        "mod_quiz_process_attempt", &args).await?;
    Ok(data.get("state").and_then(|v| v.as_str()).unwrap_or("unknown").to_string())
}
