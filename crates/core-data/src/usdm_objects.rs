use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::usdm_values::{json_string, value_string};

pub(crate) fn collect_usdm_object_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_usdm_object_rows_at(value, "", rows);
}

fn collect_usdm_object_rows_at(value: &Value, path: &str, rows: &mut Vec<BTreeMap<String, Value>>) {
    match value {
        Value::Object(object) => {
            if object.contains_key("id") {
                rows.push(usdm_object_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_object_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_object_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn usdm_object_row(object: &Value, path: &str) -> BTreeMap<String, Value> {
    let id = value_string(object.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(object.get("instanceType")),
    );
    row.insert("id".to_owned(), Value::String(id.clone()));
    row.insert("name".to_owned(), json_string(object.get("name")));
    row.insert(
        "usdm_id_contains_space".to_owned(),
        Value::Bool(id.contains(' ')),
    );
    row.insert(
        "usdm_duplicate_name_for_class".to_owned(),
        Value::Bool(false),
    );
    row.insert("usdm_duplicate_id".to_owned(), Value::Bool(false));
    row
}

pub(crate) fn apply_usdm_object_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut name_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for row in rows.iter() {
        let instance_type = row
            .get("instanceType")
            .and_then(value_string)
            .unwrap_or_default();
        let name = row.get("name").and_then(value_string).unwrap_or_default();
        if !instance_type.is_empty() && !name.is_empty() {
            *name_counts.entry((instance_type, name)).or_insert(0) += 1;
        }
        let id = row.get("id").and_then(value_string).unwrap_or_default();
        if !id.is_empty() {
            *id_counts.entry(id).or_insert(0) += 1;
        }
    }

    for row in rows.iter_mut() {
        let instance_type = row
            .get("instanceType")
            .and_then(value_string)
            .unwrap_or_default();
        let name = row.get("name").and_then(value_string).unwrap_or_default();
        let duplicate_name = !instance_type.is_empty()
            && !name.is_empty()
            && name_counts
                .get(&(instance_type, name))
                .is_some_and(|count| *count > 1);
        let id = row.get("id").and_then(value_string).unwrap_or_default();
        let duplicate_id = !id.is_empty() && id_counts.get(&id).is_some_and(|count| *count > 1);
        row.insert(
            "usdm_duplicate_name_for_class".to_owned(),
            Value::Bool(duplicate_name),
        );
        row.insert("usdm_duplicate_id".to_owned(), Value::Bool(duplicate_id));
    }
}
