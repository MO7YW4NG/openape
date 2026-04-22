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

/// Percent-encode a string for use in URL path segments.
/// Encodes all bytes except unreserved ASCII (alphanumeric, `-`, `.`, `_`, `~`).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Percent-decode a string (handles %XX sequences).
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'0'..=b'9' => Some(b - b'0'),
        _ => None,
    }
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Strip HTML tags but preserve line breaks from <br> and </p>.
pub fn strip_html_keep_lines(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let text = Regex::new(r"<br\s*/?>").unwrap()
        .replace_all(html, "\n");
    let text = Regex::new(r"</p>").unwrap()
        .replace_all(&text, "\n");
    let text = Regex::new(r"<[^>]+>").unwrap()
        .replace_all(&text, "");
    let text = text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    Regex::new(r"\n{3,}").unwrap()
        .replace_all(text.trim(), "\n\n")
        .trim()
        .to_string()
}

/// Parse quiz question HTML into structured text and options.
pub fn parse_question_html(html: &str) -> (String, Vec<String>) {
    let qtext_re = Regex::new(r#"(?s)<div class="qtext">(.*?)</div>\s*</div>"#).unwrap();
    let text = match qtext_re.captures(html) {
        Some(caps) => strip_html_keep_lines(&caps[1]),
        None => String::new(),
    };

    let option_re = Regex::new(r#"(?s)data-region="answer-label">(.*?)</div>\s*</div>"#).unwrap();
    let options: Vec<String> = option_re.captures_iter(html)
        .filter_map(|caps| {
            let stripped = strip_html_keep_lines(&caps[1]);
            if stripped.is_empty() { None } else { Some(stripped) }
        })
        .collect();

    (text, options)
}

/// Parse the saved/selected answer from quiz question HTML.
pub fn parse_saved_answer(html: &str) -> Option<serde_json::Value> {
    // Single choice: <input type="radio" ... value="N" ... checked="checked">
    let radio_re = Regex::new(r#"<input type="radio"[^>]*value="(\d+)"[^>]*checked="checked""#).unwrap();
    if let Some(caps) = radio_re.captures(html) {
        if &caps[1] != "-1" {
            return Some(serde_json::Value::String(caps[1].to_string()));
        }
    }

    // Multiple choice: checkboxes with checked="checked"
    let checkbox_re = Regex::new(r#"<input type="checkbox"[^>]*name="[^"]*choice(\d+)"[^>]*checked="checked""#).unwrap();
    let checked: Vec<String> = checkbox_re.captures_iter(html)
        .map(|caps| caps[1].to_string())
        .collect();
    if !checked.is_empty() {
        return Some(serde_json::Value::String(checked.join(",")));
    }

    // Short answer: <input ... name="...:_answer" ... value="text">
    let text_re = Regex::new(r#"<input[^>]*(?:name="[^"]*:_answer"|type="text")[^>]*(?:name="[^"]*:_answer"|type="text")[^>]*value="([^"]*)""#).unwrap();
    if let Some(caps) = text_re.captures(html) {
        if !caps[1].is_empty() {
            return Some(serde_json::Value::String(caps[1].to_string()));
        }
    }

    None
}
