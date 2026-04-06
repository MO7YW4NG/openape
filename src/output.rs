use crate::OutputFormat;

/// Format and output data in the specified format.
pub fn format_and_output(
    data: &[serde_json::Value],
    format: OutputFormat,
    meta: Option<&serde_json::Value>,
) {
    match format {
        OutputFormat::Json => {
            if let Some(m) = meta {
                println!("{}", serde_json::to_string(m).unwrap());
            }
            for item in data {
                println!("{}", serde_json::to_string(item).unwrap());
            }
            std::process::exit(0);
        }
        OutputFormat::Csv => {
            if data.is_empty() {
                return;
            }
            println!("{}", format_as_csv(data));
        }
        OutputFormat::Table => {
            if data.is_empty() {
                println!("No data");
                return;
            }
            println!("{}", format_as_table(data));
        }
        OutputFormat::Silent => {}
    }
}

/// Format data as CSV string.
pub fn format_as_csv(data: &[serde_json::Value]) -> String {
    let fields: Vec<String> = data[0]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    let mut rows = Vec::new();
    rows.push(fields.join(","));

    for item in data {
        let row: Vec<String> = fields
            .iter()
            .map(|f| {
                let val = item.get(f);
                match val {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(v) => {
                        let s = v.to_string().trim_matches('"').to_string();
                        if s.contains(',') || s.contains('"') || s.contains('\n') {
                            format!("\"{}\"", s.replace('"', "\"\""))
                        } else {
                            s
                        }
                    }
                }
            })
            .collect();
        rows.push(row.join(","));
    }

    rows.join("\n")
}

/// Format data as ASCII table.
pub fn format_as_table(data: &[serde_json::Value]) -> String {
    let all_fields: Vec<String> = {
        let mut set: Vec<String> = data
            .iter()
            .filter_map(|item| item.as_object())
            .flat_map(|o| o.keys().cloned().collect::<Vec<_>>())
            .collect();
        set.sort();
        set.dedup();
        set
    };

    // Calculate column widths
    let mut widths: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for field in &all_fields {
        let mut w = field.len() + 2;
        for item in data {
            let val = item
                .get(field)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            w = w.max(val.len() + 2);
        }
        widths.insert(field.clone(), w);
    }

    let header: String = all_fields
        .iter()
        .map(|f| {
            let w = widths.get(f).copied().unwrap_or(f.len() + 2);
            format!("{:width$}", f, width = w)
        })
        .collect::<Vec<_>>()
        .join(" | ");

    let separator: String = all_fields
        .iter()
        .map(|f| {
            let w = widths.get(f).copied().unwrap_or(f.len() + 2);
            "-".repeat(w - 1)
        })
        .collect::<Vec<_>>()
        .join("-+-");

    let rows: Vec<String> = data
        .iter()
        .map(|item| {
            all_fields
                .iter()
                .map(|f| {
                    let w = widths.get(f).copied().unwrap_or(f.len() + 2);
                    let val = item
                        .get(f)
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    format!("{:width$}", val, width = w)
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect();

    let mut lines = vec![header, separator];
    lines.extend(rows);
    lines.join("\n")
}
