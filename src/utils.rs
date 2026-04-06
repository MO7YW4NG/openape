use regex::Regex;

/// Strip HTML tags from a string, preserving text content.
pub fn strip_html_tags(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let tag_re = Regex::new(r"<[^>]*>").unwrap();
    let numeric_re = Regex::new(r"&#(\d+);").unwrap();

    let text = tag_re.replace_all(html, "");
    let text = text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let text = numeric_re.replace_all(&text, |caps: &regex::Captures| {
        caps[1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    });
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract clean course name from Moodle fullname.
/// Removes mlang tags and course codes.
pub fn extract_course_name(fullname: &str) -> String {
    if fullname.is_empty() {
        return String::new();
    }
    let mlang_re = Regex::new(r"\{mlang[^}]*\}").unwrap();
    let name_re = Regex::new(r"\d{4,}([^(\[\-]+)").unwrap();
    let cleaned = mlang_re.replace_all(fullname, "");
    match name_re.captures(&cleaned) {
        Some(caps) => caps[1].trim().to_string(),
        None => fullname.to_string(),
    }
}

/// Sanitize filename by removing invalid characters.
pub fn sanitize_filename(name: &str, max_length: usize) -> String {
    let re = Regex::new(r#"[<>:"/\\|?*]"#).unwrap();
    let cleaned = re.replace_all(name, "_");
    let re2 = Regex::new(r"\s+").unwrap();
    let result = re2.replace_all(&cleaned, "_");
    result.chars().take(max_length).collect()
}

/// Format file size to KB.
pub fn format_file_size(bytes: u64, decimals: usize) -> String {
    format!("{:.prec$}", bytes as f64 / 1024.0, prec = decimals)
}

/// Format Moodle timestamp to localized string.
pub fn format_moodle_date(timestamp: Option<i64>) -> String {
    match timestamp {
        Some(ts) if ts > 0 => {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "無期限".to_string())
        }
        _ => "無期限".to_string(),
    }
}
