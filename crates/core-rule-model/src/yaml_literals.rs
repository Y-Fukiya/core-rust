use std::collections::VecDeque;

use serde_json::Value;

use crate::Result;

pub(crate) fn yaml_condition_value_literals(source: &str) -> VecDeque<String> {
    let mut values = VecDeque::new();
    let mut check_indent = None;
    let mut value_list_indent = None;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if let Some(indent_start) = check_indent {
            if !trimmed.is_empty() && indent <= indent_start && !trimmed.starts_with("Check:") {
                check_indent = None;
                value_list_indent = None;
            }
        }
        if trimmed.starts_with("Check:") {
            check_indent = Some(indent);
            value_list_indent = None;
            continue;
        }
        if check_indent.is_none() {
            continue;
        }

        if let Some(list_indent) = value_list_indent {
            if trimmed.is_empty() {
                continue;
            }
            if indent <= list_indent {
                value_list_indent = None;
            } else if let Some(item) = trimmed.strip_prefix("- ") {
                if let Some(value) = yaml_boolish_scalar(item) {
                    values.push_back(value.to_owned());
                }
                continue;
            }
        }

        let Some(rest) = trimmed.strip_prefix("value:") else {
            continue;
        };
        if rest.trim().is_empty() {
            value_list_indent = Some(indent);
        } else if let Some(value) = yaml_boolish_scalar(rest) {
            values.push_back(value.to_owned());
        }
    }
    values
}

fn yaml_boolish_scalar(value: &str) -> Option<&str> {
    let scalar = strip_yaml_comment(value).trim();
    if scalar.starts_with(['"', '\'']) {
        return None;
    }
    matches!(
        scalar,
        "Y" | "y" | "N" | "n" | "Yes" | "yes" | "YES" | "No" | "no" | "NO"
    )
    .then_some(scalar)
}

fn strip_yaml_comment(value: &str) -> &str {
    let mut quoted = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if let Some(quote) = quoted {
            if quote == '"' && ch == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if ch == quote && !escaped {
                quoted = None;
            }
            escaped = false;
            continue;
        }
        if matches!(ch, '"' | '\'') {
            quoted = Some(ch);
            continue;
        }
        if ch == '#'
            && value[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            return &value[..index];
        }
    }
    value
}

pub(crate) fn normalize_yaml_condition_value_literals(
    value: &mut Value,
    value_literals: &mut VecDeque<String>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            if object.contains_key("operator") && object.contains_key("value") {
                if let Some(value) = object.get_mut("value") {
                    normalize_yaml_condition_value(value, value_literals);
                }
            }
            for child in object.values_mut() {
                normalize_yaml_condition_value_literals(child, value_literals)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_yaml_condition_value_literals(value, value_literals)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_yaml_condition_value(value: &mut Value, value_literals: &mut VecDeque<String>) {
    match value {
        Value::Bool(bool_value) => {
            *value = Value::String(
                value_literals
                    .pop_front()
                    .unwrap_or_else(|| bool_value.to_string()),
            );
        }
        Value::Array(values) => {
            for value in values {
                normalize_yaml_condition_value(value, value_literals);
            }
        }
        _ => {}
    }
}
