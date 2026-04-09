use anyhow::Result;
use crate::Cli;
use crate::moodle::course::get_enrolled_courses_api;
use crate::output::format_and_output;
use crate::utils::format_moodle_date;
use super::{ApiCtx, level_to_classification};

pub async fn run(cmd: &crate::CoursesCommands, cli: &Cli) -> Result<()> {
    let ctx = ApiCtx::build(cli)?;

    match cmd {
        crate::CoursesCommands::List { incomplete_only, level } => {
            let classification = level_to_classification(*level);
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, classification).await?;

            let filtered: Vec<_> = if *incomplete_only {
                courses.into_iter().filter(|c| c.progress.unwrap_or(0) < 100).collect()
            } else {
                courses
            };

            let items: Vec<serde_json::Value> = filtered.iter()
                .map(|c| serde_json::json!({
                    "id": c.id,
                    "fullname": c.fullname,
                    "shortname": c.shortname,
                    "progress": c.progress,
                    "startdate": format_moodle_date(c.startdate),
                    "enddate": format_moodle_date(c.enddate),
                }))
                .collect();

            format_and_output(&items, ctx.output, None);
        }

        crate::CoursesCommands::Info { course_id } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "all").await?;
            let course = courses.iter().find(|c| c.id == *course_id)
                .ok_or_else(|| anyhow::anyhow!("Course not found: {}", course_id))?;

            let item = serde_json::to_value(course)?;
            format_and_output(&[item], ctx.output, None);
        }

        crate::CoursesCommands::Progress { course_id } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "all").await?;
            let course = courses.iter().find(|c| c.id == *course_id)
                .ok_or_else(|| anyhow::anyhow!("Course not found: {}", course_id))?;

            let item = serde_json::json!({
                "courseId": course.id,
                "courseName": course.fullname,
                "progress": course.progress.unwrap_or(0),
                "startDate": format_moodle_date(course.startdate),
                "endDate": format_moodle_date(course.enddate),
            });

            format_and_output(&[item], ctx.output, None);
        }

        crate::CoursesCommands::Syllabus { course_id } => {
            let courses = get_enrolled_courses_api(&ctx.client, &ctx.session, "all").await?;
            let course = courses.iter().find(|c| c.id == *course_id)
                .ok_or_else(|| anyhow::anyhow!("Course not found: {}", course_id))?;

            let syllabus = fetch_syllabus(&ctx.client, &course.shortname).await;

            let mut result = serde_json::json!({
                "courseId": course.id,
                "shortname": course.shortname,
                "fullname": course.fullname,
            });

            match syllabus {
                Some(s) => {
                    if let serde_json::Value::Object(ref mut map) = result {
                        if let serde_json::Value::Object(extra) = s {
                            map.extend(extra);
                        }
                    }
                }
                None => {
                    ctx.log.warn(&format!("Syllabus not found for course: {}", course.shortname));
                    if let serde_json::Value::Object(ref mut map) = result {
                        map.insert("note".to_string(), serde_json::json!("Syllabus not available from CMAP"));
                    }
                }
            }

            format_and_output(&[result], ctx.output, None);
        }
    }

    Ok(())
}

/// Fetch course syllabus from CYCU CMAP GWT-RPC endpoint.
async fn fetch_syllabus(client: &reqwest::Client, shortname: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = shortname.splitn(2, '_').collect();
    if parts.len() < 2 {
        return None;
    }
    let (year_term, op_code) = (parts[0], parts[1]);

    let gwt_body = format!(
        "7|0|8|https://cmap.cycu.edu.tw:8443/Syllabus/syllabus/|339796D6E7B561A6465F5E9B5F4943FA|\
        com.sanfong.syllabus.shared.SyllabusClientService|findClassTargetByYearAndOpCode|\
        java.lang.String/2004016611|{}|{}|zh_TW|1|2|3|4|3|5|5|5|6|7|8|",
        year_term, op_code
    );

    let resp = client
        .post("https://cmap.cycu.edu.tw:8443/Syllabus/syllabus/syllabusClientService")
        .header("X-GWT-Permutation", "339796D6E7B561A6465F5E9B5F4943FA")
        .header("Accept", "text/x-gwt-rpc, */*; q=0.01")
        .header("Content-Type", "text/x-gwt-rpc; charset=UTF-8")
        .body(gwt_body)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return Some(serde_json::json!({
            "error": format!("HTTP {}", resp.status()),
        }));
    }

    let raw = resp.text().await.ok()?;
    if !raw.starts_with("//OK") {
        return Some(serde_json::json!({
            "error": "Invalid GWT-RPC response",
            "rawResponse": &raw[..200.min(raw.len())],
        }));
    }

    // Parse GWT string table
    let content = &raw[4..];
    let mut string_table: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in content.chars() {
        if escaped {
            match ch {
                'n' => current.push('\n'),
                'r' => current.push('\r'),
                't' => current.push('\t'),
                '"' => current.push('"'),
                '\\' => current.push('\\'),
                '0' => current.push('\0'),
                other => current.push(other),
            }
            escaped = false;
            continue;
        }
        if ch == '\\' { escaped = true; continue; }
        if ch == '"' {
            in_string = !in_string;
            if !in_string && !current.is_empty() {
                string_table.push(std::mem::take(&mut current));
            }
            continue;
        }
        if in_string { current.push(ch); }
    }

    // Extract schedule from string table (week numbers 1-18)
    let date_pattern = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").ok()?;
    let week_pattern = regex::Regex::new(r"^[1-9]$|^1[0-8]$").ok()?;

    let mut schedule: Vec<serde_json::Value> = Vec::new();
    let mut processed: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for i in 0..string_table.len() {
        let s = &string_table[i];
        if !week_pattern.is_match(s) || processed.contains(&i) { continue; }

        let week = s.clone();
        let mut date = String::new();
        let mut title = String::new();

        if i > 0 && !processed.contains(&(i - 1)) {
            title = string_table[i - 1].clone();
        }

        let is_edge = week == "1" || week == "2" || week == "18";
        if is_edge {
            let max_lookback = if week == "18" { 15 } else { 6 };
            for j in (i.saturating_sub(max_lookback)..i).rev() {
                if date_pattern.is_match(&string_table[j]) && !processed.contains(&j) {
                    date = string_table[j].clone();
                    processed.insert(j);
                    break;
                }
            }
        } else {
            for j in (i + 1)..string_table.len().min(i + 10) {
                if week_pattern.is_match(&string_table[j]) && !processed.contains(&j) {
                    for k in (j.saturating_sub(6)..j).rev() {
                        if date_pattern.is_match(&string_table[k]) && !processed.contains(&k) {
                            date = string_table[k].clone();
                            processed.insert(k);
                            break;
                        }
                    }
                    break;
                }
            }
        }

        // Clean title
        let title = title.trim()
            .replace(['\r', '\n'], " ")
            .trim_end_matches(',')
            .trim()
            .chars()
            .take(200)
            .collect::<String>();

        if title.len() > 1 {
            // Infer date if missing
            let date = if date.is_empty() {
                if let Some(last) = schedule.last() {
                    if let Some(last_date) = last.get("date").and_then(|d| d.as_str()) {
                        infer_next_week_date(last_date)
                    } else { date }
                } else { date }
            } else { date };

            schedule.push(serde_json::json!({ "week": week, "date": date, "title": title }));
        }

        processed.insert(i);
    }

    schedule.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        da.cmp(db)
    });

    let mut result = serde_json::json!({
        "yearTerm": year_term,
        "opCode": op_code,
        "url": format!("https://cmap.cycu.edu.tw:8443/Syllabus/CoursePreview.html?yearTerm={}&opCode={}&locale=zh_TW", year_term, op_code),
        "schedule": schedule,
    });

    // Try to find instructor
    for s in &string_table {
        if s.contains("教授") || s.contains("老師") || s.contains("教師") || s.contains("Instructor") {
            if let serde_json::Value::Object(ref mut map) = result {
                map.insert("instructor".to_string(), serde_json::json!(s));
            }
            break;
        }
    }

    Some(result)
}

fn infer_next_week_date(last_date: &str) -> String {
    use std::str::FromStr;
    let parts: Vec<&str> = last_date.splitn(3, '-').collect();
    if parts.len() != 3 { return String::new(); }
    let (y, m, d) = (
        i32::from_str(parts[0]).unwrap_or(2024),
        u32::from_str(parts[1]).unwrap_or(1),
        u32::from_str(parts[2]).unwrap_or(1),
    );
    // Simple +7 days calculation
    let days_in_month = [0u32, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let feb_days = if leap { 29u32 } else { 28 };
    let dim = if m == 2 { feb_days } else { days_in_month[m as usize] };
    let (ny, nm, nd) = if d + 7 > dim {
        let nd = d + 7 - dim;
        if m == 12 { (y + 1, 1, nd) } else { (y, m + 1, nd) }
    } else {
        (y, m, d + 7)
    };
    format!("{:04}-{:02}-{:02}", ny, nm, nd)
}
