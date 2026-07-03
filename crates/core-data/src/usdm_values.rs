use serde_json::Value;

pub(crate) fn json_string(value: Option<&Value>) -> Value {
    value
        .and_then(value_string)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

pub(crate) fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn value_exists(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}

pub(crate) fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(value_string).collect())
        .unwrap_or_default()
}

pub(crate) fn format_string_list(values: &[String]) -> String {
    format!("[{}]", values.join(", "))
}

pub(crate) fn format_semicolon_list(values: &[String]) -> String {
    format!("[{}]", values.join("; "))
}

pub(crate) fn format_code(code: Option<&Value>) -> String {
    let Some(code) = code else {
        return String::new();
    };
    let decode = code
        .get("decode")
        .and_then(value_string)
        .unwrap_or_default();
    let code_value = code.get("code").and_then(value_string).unwrap_or_default();
    if code_value.is_empty() {
        decode
    } else {
        format!("{decode} ({code_value})")
    }
}

pub(crate) fn format_quantity_single(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_default();
    if unit.is_empty() {
        quantity
    } else {
        format!("{quantity} {unit}")
    }
}

pub(crate) fn format_quantity_single_with_missing_unit(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_else(|| "unit not specified".to_owned());
    format!("{quantity} {unit}")
}
