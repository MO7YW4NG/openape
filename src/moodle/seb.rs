use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Fetch and compute the SEB config key for a quiz by cmid.
/// Requires session cookies to be pre-loaded in the reqwest client.
pub async fn fetch_seb_config_key(
    client: &reqwest::Client,
    base_url: &str,
    cmid: u64,
) -> anyhow::Result<String> {
    let response = client
        .get(format!("{base_url}/mod/quiz/accessrule/seb/config.php"))
        .query(&[("cmid", cmid)])
        .send()
        .await?;
    let text = response.text().await?;
    if text.trim().is_empty() || !text.contains("<plist") {
        anyhow::bail!(
            "No SEB config available for cmid={} (response: {}...)",
            cmid,
            &text[..text.len().min(120)]
        );
    }
    Ok(compute_config_key(&text))
}

/// Parse the subset of plist XML used by SEB configuration files.
fn parse_plist(xml: &str) -> Value {
    let mut pos = 0;
    let bytes = xml.as_bytes();
    let len = bytes.len();

    fn skip_ws(bytes: &[u8], pos: &mut usize, len: usize) {
        while *pos < len
            && (bytes[*pos] == b' '
                || bytes[*pos] == b'\n'
                || bytes[*pos] == b'\r'
                || bytes[*pos] == b'\t')
        {
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
        let value = String::from_utf8_lossy(&bytes[start..*pos]).to_string();
        if *pos < len {
            *pos += 1;
        }
        value
    }

    fn skip_close_tag(bytes: &[u8], pos: &mut usize, len: usize) {
        while *pos < len && bytes[*pos] != b'>' {
            *pos += 1;
        }
        if *pos < len {
            *pos += 1;
        }
    }

    fn parse_value(bytes: &[u8], pos: &mut usize, len: usize) -> Value {
        skip_ws(bytes, pos, len);
        if *pos >= len {
            return Value::Null;
        }

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
            let value = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::Number(serde_json::Number::from(
                value.trim().parse::<i64>().unwrap_or(0),
            ))
        } else if expect(bytes, pos, len, b"<real>") {
            let value = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::Number(
                serde_json::Number::from_f64(value.trim().parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(serde_json::Number::from(0)),
            )
        } else if expect(bytes, pos, len, b"<string>") {
            let value = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::String(value)
        } else if expect(bytes, pos, len, b"<data>") || expect(bytes, pos, len, b"<date>") {
            let value = read_until(bytes, pos, len, b'<');
            skip_close_tag(bytes, pos, len);
            Value::String(value.trim().to_string())
        } else if expect(bytes, pos, len, b"<array>") {
            let mut values = Vec::new();
            loop {
                skip_ws(bytes, pos, len);
                if *pos >= len || expect(bytes, pos, len, b"</array>") {
                    break;
                }
                values.push(parse_value(bytes, pos, len));
            }
            Value::Array(values)
        } else if expect(bytes, pos, len, b"<dict>") {
            let mut values = Map::new();
            loop {
                skip_ws(bytes, pos, len);
                if *pos >= len || expect(bytes, pos, len, b"</dict>") {
                    break;
                }
                if expect(bytes, pos, len, b"<key>") {
                    let key = read_until(bytes, pos, len, b'<');
                    skip_close_tag(bytes, pos, len);
                    values.insert(key, parse_value(bytes, pos, len));
                } else {
                    *pos += 1;
                }
            }
            Value::Object(values)
        } else {
            *pos += 1;
            Value::Null
        }
    }

    loop {
        skip_ws(bytes, &mut pos, len);
        if pos >= len {
            break;
        }
        let starts_with = |pos: usize, prefix: &[u8]| {
            pos + prefix.len() <= len && &bytes[pos..pos + prefix.len()] == prefix
        };
        if starts_with(pos, b"<?") || starts_with(pos, b"<!") || starts_with(pos, b"<plist") {
            while pos < len && bytes[pos] != b'>' {
                pos += 1;
            }
            if pos < len {
                pos += 1;
            }
        } else {
            break;
        }
    }

    parse_value(bytes, &mut pos, len)
}

fn sort_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for value in map.values_mut() {
                sort_keys(value);
            }
            let mut pairs: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            pairs.sort_by_key(|pair| pair.0.to_lowercase());
            *map = pairs.into_iter().collect();
        }
        Value::Array(values) => {
            for value in values {
                sort_keys(value);
            }
        }
        _ => {}
    }
}

fn cleanup(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("originatorVersion");
            map.retain(|_, value| !matches!(value, Value::Object(inner) if inner.is_empty()));
            for value in map.values_mut() {
                cleanup(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                cleanup(value);
            }
        }
        _ => {}
    }
}

/// Compute the SEB Config Key from a plist XML string.
pub fn compute_config_key(plist_xml: &str) -> String {
    let mut value = parse_plist(plist_xml);
    cleanup(&mut value);
    sort_keys(&mut value);
    hex::encode(Sha256::digest(
        serde_json::to_string(&value).unwrap().as_bytes(),
    ))
}

/// Compute the X-SafeExamBrowser-ConfigKeyHash header value.
pub fn compute_config_key_hash(url: &str, config_key: &str) -> String {
    hex::encode(Sha256::digest(format!("{url}{config_key}").as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_key_is_stable_across_dict_order_and_ignored_metadata() {
        let first = "<plist><dict><key>b</key><integer>2</integer><key>originatorVersion</key><string>x</string><key>a</key><true/></dict></plist>";
        let second =
            "<plist><dict><key>a</key><true/><key>b</key><integer>2</integer></dict></plist>";
        assert_eq!(compute_config_key(first), compute_config_key(second));
    }
}
