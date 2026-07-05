use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::usdm_common::named_usdm_object_name;
use crate::usdm_values::{json_string, value_string};

pub(crate) fn collect_usdm_identifier_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let organizations = version
            .get("organizations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut version_rows = Vec::new();
        collect_named_identifier_rows(
            version.get("studyIdentifiers"),
            "StudyIdentifier",
            &format!("/study/versions/{version_index}/studyIdentifiers"),
            &organizations,
            &mut version_rows,
        );
        collect_named_identifier_rows(
            version.get("referenceIdentifiers"),
            "ReferenceIdentifier",
            &format!("/study/versions/{version_index}/referenceIdentifiers"),
            &organizations,
            &mut version_rows,
        );
        collect_nested_identifiers(
            version.get("administrableProducts"),
            "AdministrableProductIdentifier",
            &format!("/study/versions/{version_index}/administrableProducts"),
            &organizations,
            &mut version_rows,
        );
        collect_nested_identifiers(
            version.get("medicalDevices"),
            "MedicalDeviceIdentifier",
            &format!("/study/versions/{version_index}/medicalDevices"),
            &organizations,
            &mut version_rows,
        );
        apply_identifier_duplicate_flags(&mut version_rows);
        rows.extend(version_rows);
    }
}

fn collect_named_identifier_rows(
    identifiers: Option<&Value>,
    instance_type: &str,
    base_path: &str,
    organizations: &[Value],
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(identifiers) = identifiers.and_then(Value::as_array) else {
        return;
    };
    for (index, identifier) in identifiers.iter().enumerate() {
        rows.push(usdm_identifier_row(
            identifier,
            instance_type,
            &format!("{base_path}/{index}"),
            organizations,
        ));
    }
}

fn collect_nested_identifiers(
    parents: Option<&Value>,
    instance_type: &str,
    base_path: &str,
    organizations: &[Value],
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(parents) = parents.and_then(Value::as_array) else {
        return;
    };
    for (parent_index, parent) in parents.iter().enumerate() {
        collect_named_identifier_rows(
            parent.get("identifiers"),
            instance_type,
            &format!("{base_path}/{parent_index}/identifiers"),
            organizations,
            rows,
        );
    }
}

fn usdm_identifier_row(
    identifier: &Value,
    instance_type: &str,
    path: &str,
    organizations: &[Value],
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    let scope_id =
        value_string(identifier.get("scopeId").unwrap_or(&Value::Null)).unwrap_or_default();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        Value::String(instance_type.to_owned()),
    );
    row.insert("id".to_owned(), json_string(identifier.get("id")));
    row.insert("text".to_owned(), json_string(identifier.get("text")));
    row.insert("scopeId".to_owned(), Value::String(scope_id.clone()));
    row.insert(
        "Organization.name".to_owned(),
        organization_name(organizations, &scope_id)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "type.code".to_owned(),
        json_string(identifier.get("type").and_then(|value| value.get("code"))),
    );
    row.insert(
        "type.decode".to_owned(),
        json_string(identifier.get("type").and_then(|value| value.get("decode"))),
    );
    row
}

fn apply_identifier_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut study_scope_counts: HashMap<String, usize> = HashMap::new();
    let mut text_scope_counts: HashMap<(String, String, String), usize> = HashMap::new();
    for row in rows.iter() {
        let instance_type = row
            .get("instanceType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let scope_id = row
            .get("scopeId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let text = row.get("text").and_then(value_string).unwrap_or_default();
        if instance_type == "StudyIdentifier" && !scope_id.is_empty() {
            *study_scope_counts.entry(scope_id.clone()).or_insert(0) += 1;
        }
        if !text.is_empty() && !scope_id.is_empty() {
            *text_scope_counts
                .entry((instance_type, scope_id, text))
                .or_insert(0) += 1;
        }
    }

    for row in rows.iter_mut() {
        let instance_type = row
            .get("instanceType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let scope_id = row
            .get("scopeId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let text = row.get("text").and_then(value_string).unwrap_or_default();
        let study_duplicate = instance_type == "StudyIdentifier"
            && study_scope_counts
                .get(&scope_id)
                .is_some_and(|count| *count > 1);
        let text_scope_duplicate = text_scope_counts
            .get(&(instance_type, scope_id, text))
            .is_some_and(|count| *count > 1);
        row.insert(
            "study_identifier_scope_duplicate".to_owned(),
            Value::Bool(study_duplicate),
        );
        row.insert(
            "identifier_text_scope_duplicate".to_owned(),
            Value::Bool(text_scope_duplicate),
        );
    }
}

fn organization_name(organizations: &[Value], id: &str) -> Option<String> {
    named_usdm_object_name(organizations, id)
}
