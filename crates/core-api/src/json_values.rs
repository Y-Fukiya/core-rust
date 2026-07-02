use serde_json::Value;

pub(crate) fn json_report_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn json_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn json_distinct_value_string(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) => value
            .as_f64()
            .map(canonical_numeric_string)
            .or_else(|| Some(value.to_string())),
        _ => json_scalar_string(value),
    }
}

fn canonical_numeric_string(value: f64) -> String {
    let formatted = format!("{value:.12}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
