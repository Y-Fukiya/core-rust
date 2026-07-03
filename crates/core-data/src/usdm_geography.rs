use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::usdm_values::{format_code, format_semicolon_list, json_string, value_string};

pub(crate) fn collect_usdm_geographic_scope_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    collect_usdm_geographic_scope_rows_at(value, "", rows);
}

fn collect_usdm_geographic_scope_rows_at(
    value: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if object
                .get("instanceType")
                .and_then(Value::as_str)
                .is_some_and(|instance_type| instance_type == "GeographicScope")
            {
                rows.push(usdm_geographic_scope_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_geographic_scope_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_geographic_scope_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_usdm_governance_date_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(documented_by) = value
        .get("study")
        .and_then(|study| study.get("documentedBy"))
    else {
        return;
    };

    for (document_index, document) in values_as_slice(documented_by).iter().enumerate() {
        let Some(versions) = document.get("versions").and_then(Value::as_array) else {
            continue;
        };
        for (version_index, version) in versions.iter().enumerate() {
            let Some(date_values) = version.get("dateValues").and_then(Value::as_array) else {
                continue;
            };
            let global_duplicate_types = governance_date_global_duplicate_types(date_values);
            for (date_index, date_value) in date_values.iter().enumerate() {
                rows.push(usdm_governance_date_row(
                    date_value,
                    document,
                    version,
                    &global_duplicate_types,
                    &format!(
                        "/study/documentedBy/{document_index}/versions/{version_index}/dateValues/{date_index}"
                    ),
                ));
            }
        }
    }
}

fn values_as_slice(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(values) => values.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

fn usdm_geographic_scope_row(scope: &Value, path: &str) -> BTreeMap<String, Value> {
    let type_code = scope
        .get("type")
        .and_then(|code| code.get("code"))
        .and_then(value_string);
    let has_code = scope.get("code").is_some_and(|code| !code.is_null());
    let invalid_scope = (type_code.as_deref() == Some("C68846")) == has_code;
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(scope.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(scope.get("id")));
    row.insert("name".to_owned(), json_string(scope.get("name")));
    row.insert(
        "type.code".to_owned(),
        type_code.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "type.decode".to_owned(),
        json_string(scope.get("type").and_then(|code| code.get("decode"))),
    );
    row.insert(
        "code.standardCode.code".to_owned(),
        json_string(
            scope
                .get("code")
                .and_then(|code| code.get("standardCode"))
                .and_then(|code| code.get("code")),
        ),
    );
    row.insert(
        "code.standardCode.decode".to_owned(),
        json_string(
            scope
                .get("code")
                .and_then(|code| code.get("standardCode"))
                .and_then(|code| code.get("decode")),
        ),
    );
    row.insert(
        "geographic_scope_global_code_mismatch".to_owned(),
        Value::Bool(invalid_scope),
    );
    row
}

fn governance_date_global_duplicate_types(date_values: &[Value]) -> HashSet<String> {
    let mut counts = HashMap::<String, usize>::new();
    let mut global_types = HashSet::new();
    for date_value in date_values {
        let Some(type_code) = date_value
            .get("type")
            .and_then(|code| code.get("code"))
            .and_then(value_string)
        else {
            continue;
        };
        *counts.entry(type_code.clone()).or_insert(0) += 1;
        if date_value
            .get("geographicScopes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|scope| {
                scope
                    .get("type")
                    .and_then(|code| code.get("code"))
                    .and_then(Value::as_str)
                    == Some("C68846")
            })
        {
            global_types.insert(type_code);
        }
    }
    counts
        .into_iter()
        .filter_map(|(type_code, count)| {
            (count > 1 && global_types.contains(&type_code)).then_some(type_code)
        })
        .collect()
}

fn usdm_governance_date_row(
    date_value: &Value,
    document: &Value,
    document_version: &Value,
    global_duplicate_types: &HashSet<String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let type_code = date_value
        .get("type")
        .and_then(|code| code.get("code"))
        .and_then(value_string);
    let geographic_scopes = date_value
        .get("geographicScopes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(format_usdm_scope_id_type)
        .collect::<Vec<_>>();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(date_value.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(date_value.get("id")));
    row.insert(
        "StudyDefinitionDocument.id".to_owned(),
        json_string(document.get("id")),
    );
    row.insert(
        "StudyDefinitionDocument.name".to_owned(),
        json_string(document.get("name")),
    );
    row.insert(
        "StudyDefinitionDocumentVersion.id".to_owned(),
        json_string(document_version.get("id")),
    );
    row.insert(
        "StudyDefinitionDocumentVersion.version".to_owned(),
        json_string(document_version.get("version")),
    );
    row.insert(
        "type".to_owned(),
        date_value
            .get("type")
            .map(|code| Value::String(format_code(Some(code))))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "dateValue".to_owned(),
        json_string(date_value.get("dateValue")),
    );
    row.insert(
        "geographicScopes.type".to_owned(),
        Value::String(format_semicolon_list(&geographic_scopes)),
    );
    row.insert(
        "governance_date_global_type_duplicate".to_owned(),
        Value::Bool(
            type_code
                .as_ref()
                .is_some_and(|code| global_duplicate_types.contains(code)),
        ),
    );
    row
}

fn format_usdm_scope_id_type(scope: &Value) -> Option<String> {
    let id = scope.get("id").and_then(value_string)?;
    let type_value = scope.get("type")?;
    Some(format!("{id}: {}", format_code(Some(type_value))))
}
