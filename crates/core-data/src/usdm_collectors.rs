use std::collections::BTreeMap;

use serde_json::Value;

use crate::usdm_row_builders::{
    usdm_address_row, usdm_duration_row, usdm_person_name_row, usdm_range_row,
};

pub(crate) fn collect_usdm_duration_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_recursive_instance_rows(value, "Duration", rows, usdm_duration_row);
}

pub(crate) fn collect_usdm_range_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_recursive_instance_rows(value, "Range", rows, usdm_range_row);
}

pub(crate) fn collect_usdm_person_name_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    collect_recursive_instance_rows(value, "PersonName", rows, usdm_person_name_row);
}

pub(crate) fn collect_usdm_address_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (org_index, organization) in version
            .get("organizations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(address) = organization.get("legalAddress") {
                rows.push(usdm_address_row(
                    address,
                    &format!(
                        "/study/versions/{version_index}/organizations/{org_index}/legalAddress"
                    ),
                    organization,
                ));
            }
        }
    }
}

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
