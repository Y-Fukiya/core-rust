use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::usdm_values::value_string;

pub(crate) fn duplicate_strings(values: &[String]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for value in values {
        *counts.entry(value.as_str()).or_insert(0) += 1;
    }
    let mut duplicates = values
        .iter()
        .filter(|value| counts.get(value.as_str()).is_some_and(|count| *count > 1))
        .cloned()
        .collect::<Vec<_>>();
    duplicates.sort();
    duplicates
}

pub(crate) fn named_usdm_object_name(values: &[Value], id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    values
        .iter()
        .find(|value| value_string(value.get("id").unwrap_or(&Value::Null)).as_deref() == Some(id))
        .and_then(|value| value_string(value.get("name").unwrap_or(&Value::Null)))
}

pub(crate) fn collect_direct_ids(value: Option<&Value>) -> HashSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("id").and_then(value_string))
        .collect()
}

pub(crate) fn collect_nested_ids(value: &Value, key: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_nested_ids_at(value, key, &mut ids);
    ids
}

fn collect_nested_ids_at(value: &Value, key: &str, ids: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(values) = object.get(key).and_then(Value::as_array) {
                ids.extend(
                    values
                        .iter()
                        .filter_map(|value| value.get("id").and_then(value_string)),
                );
            }
            for child in object.values() {
                collect_nested_ids_at(child, key, ids);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_nested_ids_at(child, key, ids);
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_managed_site_ids(version: &Value) -> HashSet<String> {
    version
        .get("organizations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|organization| {
            organization
                .get("managedSites")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|site| site.get("id").and_then(value_string))
        })
        .collect()
}
