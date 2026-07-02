use std::collections::BTreeMap;

use core_rule_model::OperationSpec;
use serde_json::Value;

pub(crate) fn is_join_operation(operation: &OperationSpec) -> bool {
    operation_name(operation)
        .as_deref()
        .is_some_and(is_join_operation_name)
}

pub(crate) fn is_supported_operation_name(name: &str) -> bool {
    is_join_operation_name(name)
        || matches!(
            name,
            "filter"
                | "where"
                | "subset"
                | "derive"
                | "add_column"
                | "aggregate"
                | "group_by"
                | "group_count"
                | "record_count"
                | "sort"
                | "order_by"
                | "min"
                | "max"
                | "select"
                | "keep"
                | "project"
                | "drop"
                | "remove_columns"
                | "exclude_columns"
                | "rename"
                | "rename_columns"
                | "distinct"
                | "deduplicate"
                | "unique"
                | "row_number"
                | "rank"
                | "domain_is_custom"
                | "domain_label"
                | "study_domains"
                | "variable_count"
                | "dy"
                | "min_date"
                | "max_date"
                | "extract_metadata"
                | "dataset_names"
                | "variable_exists"
                | "expected_variables"
                | "required_variables"
                | "get_column_order_from_dataset"
                | "get_column_order_from_library"
                | "valid_codelist_dates"
                | "map"
                | "codelist_extensible"
                | "codelist_terms"
                | "split_by"
                | "get_model_column_order"
                | "get_parent_model_column_order"
                | "get_dataset_filtered_variables"
                | "get_model_filtered_variables"
                | "get_xhtml_errors"
        )
}

fn is_join_operation_name(name: &str) -> bool {
    matches!(
        name,
        "join"
            | "left_join"
            | "dataset_join"
            | "inner_join"
            | "semi_join"
            | "anti_join"
            | "merge"
            | "lookup"
            | "match_dataset"
            | "match_datasets"
    )
}

pub(crate) fn operation_name(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["operator", "name", "type", "operation"])
        .map(|value| normalize_operation_key(&value))
}

pub(crate) fn string_field(operation: &OperationSpec, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(crate) fn string_list_field(operation: &OperationSpec, keys: &[&str]) -> Option<Vec<String>> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(strings_from_value)
        .filter(|values| !values.is_empty())
}

pub(crate) fn bool_field(operation: &OperationSpec, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn string_map_field(
    operation: &OperationSpec,
    keys: &[&str],
) -> Option<BTreeMap<String, String>> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|values| !values.is_empty())
}

pub(crate) fn rename_pair(operation: &OperationSpec) -> Option<BTreeMap<String, String>> {
    let from = string_field(operation, &["from", "source", "old", "old_name"])?;
    let to = string_field(operation, &["to", "target", "new", "new_name", "as"])?;
    Some(BTreeMap::from([(from, to)]))
}

pub(crate) fn operation_value<'a>(
    operation: &'a OperationSpec,
    keys: &[&str],
) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        operation
            .fields
            .get(*key)
            .or_else(|| field_normalized(operation, key))
    })
}

pub(crate) fn operation_function_argument<'a>(
    expression: &'a str,
    names: &[&str],
) -> Option<&'a str> {
    let args = operation_function_arguments(expression, names)?;
    (args.len() == 1).then_some(args[0])
}

pub(crate) fn operation_function_arguments<'a>(
    expression: &'a str,
    names: &[&str],
) -> Option<Vec<&'a str>> {
    let expression = expression.trim();
    let open = expression.find('(')?;
    if !names
        .iter()
        .any(|name| operation_function_names_equal(&expression[..open], name))
        || !expression.ends_with(')')
    {
        return None;
    }
    Some(split_operation_commas(
        &expression[open + 1..expression.len() - 1],
    ))
}

fn operation_function_names_equal(left: &str, right: &str) -> bool {
    left.trim()
        .trim_start_matches('$')
        .eq_ignore_ascii_case(right.trim().trim_start_matches('$'))
}

fn split_operation_commas(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(expression[start..byte_index].trim());
                start = byte_index + 1;
            }
            _ => {}
        }
    }

    if !expression[start..].trim().is_empty() {
        parts.push(expression[start..].trim());
    }
    parts
}

pub(crate) fn operation_string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }
    let quote = value.chars().next()?;
    if !matches!(quote, '"' | '\'') || !value.ends_with(quote) {
        return None;
    }
    Some(
        value[1..value.len() - 1]
            .replace("\\'", "'")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
    )
}

pub(crate) fn is_quoted_literal(value: &str) -> bool {
    operation_string_literal(value).is_some()
}

pub(crate) fn clean_operation_identifier(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('$')
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

fn strings_from_value(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(value) => Some(vec![value.clone()]),
        Value::Array(values) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        ),
        _ => None,
    }
}

fn field_normalized<'a>(operation: &'a OperationSpec, key: &str) -> Option<&'a Value> {
    let normalized_key = normalize_operation_key(key);
    operation
        .fields
        .iter()
        .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized_key)
        .map(|(_key, value)| value)
}

pub(crate) fn normalize_operation_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_word = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_uppercase() {
            if previous_was_word {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
            previous_was_word = true;
        } else if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_was_word = true;
        } else {
            normalized.push('_');
            previous_was_word = false;
        }
    }

    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
