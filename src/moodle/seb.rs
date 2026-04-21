use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Fetch and compute the SEB config key for a quiz by cmid.
/// Requires session cookies to be pre-loaded in the reqwest client.
pub async fn fetch_seb_config_key(
    client: &reqwest::Client,
    base_url: &str,
    cmid: u64,
) -> anyhow::Result<String> {
    let url = format!("{}/mod/quiz/accessrule/seb/config.php?cmid={}", base_url, cmid);
    let resp = client.get(&url).send().await?;
    let text = resp.text().await?;
    if text.trim().is_empty() || !text.contains("<plist") {
        anyhow::bail!(
            "No SEB config available for cmid={} (response: {}...)",
            cmid,
            &text[..text.len().min(120)]
        );
    }
    Ok(compute_config_key(&text))
}

/// Parse a plist XML string into a serde_json Value (dict/array/bool/int/string).
/// Handles the subset used by SEB config files.
fn parse_plist(xml: &str) -> Value {
    let mut pos = 0;
    let bytes = xml.as_bytes();
    let len = bytes.len();

    fn skip_ws(bytes: &[u8], pos: &mut usize, len: usize) {
        while *pos < len && (bytes[*pos] == b' ' || bytes[*pos] == b'\n' || bytes[*pos] == b'\r' || bytes[*pos] == b'\t') {
            *pos += 1;
        }
    }

    fn expect(bytes: &[u8], pos: &mut usize, len: usize, tag: &[u8]) -> bool {
        skip_ws(bytes, pos, len);
        if *pos + tag.len() <= len && &bytes[*pos..*pos + tag.len()] == tag {
            *pos += tag.len();
            return true;
        }
        false
    }

    fn read_until(bytes: &[u8], pos: &mut usize, len: usize, end: u8) -> String {
        let start = *pos;
        while *pos < len && bytes[*pos] != end {
            *pos += 1;
        }
        let s = String::from_utf8_lossy(&bytes[start..*pos]).to_string();
        if *pos < len { *pos += 1; }
        s
    }

    // Skip the rest of a closing tag after read_until consumed the '<'.
    fn skip_close_tag(bytes: &[u8], pos: &mut usize, len: usize) {
        while *pos < len && bytes[*pos] != b'>' { *pos += 1; }
        if *pos < len { *pos += 1; }
    }

    fn parse_value(bytes: &[u8], pos: &mut usize, len: usize) -> Value {
        skip_ws(bytes, pos, len);
        if *pos >= len { return Value::Null; }

        if expect(bytes, pos, len, b"<true/>") {
            Value::Bool(true)
        } else if expect(bytes, pos, len, b"<false/>") {
            Value::Bool(false)
        } else if expect(bytes, pos, len, b"<array/>") {
            Value::Array(Vec::new())
        } else if expect(bytes, pos, len, b"<dict/>") {
            Value::Object(Map::new())
        } else if expect(bytes, pos, len, b"<string/>") {
            Value::String(String::new())
        } else if expect(bytes, pos, len, b"<integer>") {
            let val = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            let n: i64 = val.trim().parse().unwrap_or(0);
            Value::Number(serde_json::Number::from(n))
        } else if expect(bytes, pos, len, b"<real>") {
            let val = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            let n: f64 = val.trim().parse().unwrap_or(0.0);
            Value::Number(serde_json::Number::from_f64(n).unwrap_or(serde_json::Number::from(0)))
        } else if expect(bytes, pos, len, b"<string>") {
            let val = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::String(val)
        } else if expect(bytes, pos, len, b"<data>") {
            let val = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::String(val.trim().to_string())
        } else if expect(bytes, pos, len, b"<date>") {
            let val = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::String(val.trim().to_string())
        } else if expect(bytes, pos, len, b"<array>") {
            let mut arr = Vec::new();
            loop {
                skip_ws(bytes, pos, len);
                if *pos >= len { break; }
                if expect(bytes, pos, len, b"</array>") { break; }
                arr.push(parse_value(bytes, pos, len));
            }
            Value::Array(arr)
        } else if expect(bytes, pos, len, b"<dict>") {
            let mut map = Map::new();
            loop {
                skip_ws(bytes, pos, len);
                if *pos >= len { break; }
                if expect(bytes, pos, len, b"</dict>") { break; }
                if expect(bytes, pos, len, b"<key>") {
                    let key = read_until(bytes, pos, len, b'<');
                    skip_close_tag(bytes, pos, len);
                    let val = parse_value(bytes, pos, len);
                    map.insert(key, val);
                } else {
                    *pos += 1;
                }
            }
            Value::Object(map)
        } else {
            *pos += 1;
            Value::Null
        }
    }

    // Skip XML preamble: <?...?>, <!...>, and <plist ...> before parsing content.
    loop {
        skip_ws(bytes, &mut pos, len);
        if pos >= len { break; }
        if pos + 2 <= len && &bytes[pos..pos+2] == b"<?" {
            while pos < len && !(bytes[pos] == b'>' && pos > 0 && bytes[pos-1] == b'?') {
                pos += 1;
            }
            if pos < len { pos += 1; }
        } else if pos + 2 <= len && &bytes[pos..pos+2] == b"<!" {
            while pos < len && bytes[pos] != b'>' {
                pos += 1;
            }
            if pos < len { pos += 1; }
        } else if pos + 6 <= len && &bytes[pos..pos+6] == b"<plist" {
            while pos < len && bytes[pos] != b'>' {
                pos += 1;
            }
            if pos < len { pos += 1; }
        } else {
            break;
        }
    }

    parse_value(bytes, &mut pos, len)
}

/// Recursively sort all dict keys alphabetically (case-insensitive).
fn sort_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for v in map.values_mut() {
                sort_keys(v);
            }
            // ksort with case-insensitive comparison (same as PHP SORT_STRING | SORT_FLAG_CASE)
            let mut pairs: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            pairs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            *map = pairs.into_iter().collect();
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                sort_keys(v);
            }
        }
        _ => {}
    }
}

/// Remove empty dict elements and delete `originatorVersion`.
fn cleanup(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("originatorVersion");
            map.retain(|_, v| {
                if let Value::Object(inner) = v {
                    !inner.is_empty()
                } else {
                    true
                }
            });
            for v in map.values_mut() {
                cleanup(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                cleanup(v);
            }
        }
        _ => {}
    }
}

/// Compute the SEB Config Key from a plist XML string.
/// Algorithm: parse plist → remove originatorVersion → sort keys → JSON serialize → SHA256.
pub fn compute_config_key(plist_xml: &str) -> String {
    let mut value = parse_plist(plist_xml);
    cleanup(&mut value);
    sort_keys(&mut value);

    // JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE — no whitespace
    let json_str = serde_json::to_string(&value).unwrap();
    // serde_json escapes backslashes by default; PHP doesn't. Replace \\n with \n in strings.
    // But for this config, there are no backslashes so this is a no-op in practice.

    let hash = Sha256::digest(json_str.as_bytes());
    hex::encode(hash)
}

/// Compute the X-SafeExamBrowser-ConfigKeyHash header value.
/// `url` is the full request URL that Moodle's $FULLME (or fallback $CFG->wwwroot) would be.
pub fn compute_config_key_hash(url: &str, config_key: &str) -> String {
    let hash = Sha256::digest(format!("{}{}", url, config_key).as_bytes());
    hex::encode(hash)
}