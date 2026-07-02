use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) fn collect_recursive_instance_rows(
    value: &Value,
    instance_type: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
    row_fn: impl Fn(&Value, &str) -> BTreeMap<String, Value> + Copy,
) {
    collect_recursive_instance_rows_at(value, "", instance_type, rows, row_fn);
}

fn collect_recursive_instance_rows_at(
    value: &Value,
    path: &str,
    instance_type: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
    row_fn: impl Fn(&Value, &str) -> BTreeMap<String, Value> + Copy,
) {
    match value {
        Value::Object(object) => {
            if object
                .get("instanceType")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == instance_type)
            {
                rows.push(row_fn(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_recursive_instance_rows_at(child, &child_path, instance_type, rows, row_fn);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_recursive_instance_rows_at(
                    child,
                    &format!("{path}/{index}"),
                    instance_type,
                    rows,
                    row_fn,
                );
            }
        }
        _ => {}
    }
}
