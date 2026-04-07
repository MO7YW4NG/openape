use comfy_table::{Table, ContentArrangement, presets::NOTHING};
use crate::OutputFormat;

/// Format and output data in the specified format.
pub fn format_and_output(
    data: &[serde_json::Value],
    format: OutputFormat,
    _meta: Option<&serde_json::Value>,
) {
    match format {
        OutputFormat::Json => {
            for item in data {
                println!("{}", serde_json::to_string(item).unwrap());
            }
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
    let mut all_fields: Vec<String> = data
        .iter()
        .filter_map(|item| item.as_object())
        .flat_map(|o| o.keys().cloned().collect::<Vec<_>>())
        .collect();
    all_fields.sort();
    all_fields.dedup();

    let mut rows = Vec::new();
    rows.push(all_fields.join(","));

    for item in data {
        let row: Vec<String> = all_fields
            .iter()
            .map(|f| {
                let val = item.get(f);
                match val {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(v) => {
                        let s = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let s = s.trim_matches('"').to_string();
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
    let mut all_fields: Vec<String> = data
        .iter()
        .filter_map(|item| item.as_object())
        .flat_map(|o| o.keys().cloned().collect::<Vec<_>>())
        .collect();
    all_fields.sort();
    all_fields.dedup();

    let mut table = Table::new();
    table
        .load_preset(NOTHING)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(140) 
        .set_header(all_fields.iter().map(|f| f.to_uppercase()));

    for item in data {
        let mut row = Vec::new();
        for field in &all_fields {
            let val = item.get(field);
            let s = match val {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::Bool(b)) => b.to_string(),
                Some(v) => v.to_string(),
            };
            row.push(s);
        }
        table.add_row(row);
    }

    table.to_string()
}
