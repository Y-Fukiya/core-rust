use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::usdm_references::{
    collect_usdm_id_instance_types, collect_usdm_reference_keys, parameter_map_reference_invalid,
    usdm_tag_references,
};
use crate::usdm_values::{json_string, value_string};

pub(crate) fn collect_usdm_condition_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let id_types = collect_usdm_id_instance_types(value);
    collect_usdm_condition_rows_at(value, "", &id_types, rows);
}

pub(crate) fn collect_usdm_parameter_map_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let reference_keys = collect_usdm_reference_keys(value);
    collect_usdm_parameter_map_rows_at(value, "", &reference_keys, rows);
}

pub(crate) fn collect_usdm_syntax_template_text_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let dictionaries = syntax_template_dictionaries(version);
        collect_usdm_syntax_template_text_rows_at(
            version,
            &format!("/study/versions/{version_index}"),
            &dictionaries,
            rows,
        );
    }
}

fn collect_usdm_syntax_template_text_rows_at(
    value: &Value,
    path: &str,
    dictionaries: &HashMap<String, SyntaxTemplateDictionary>,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            let local_dictionaries = merged_syntax_template_dictionaries(value, dictionaries);
            if syntax_template_text_target_entity(object) {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    for (parameter_reference, parameter_name) in usdm_tag_references(text) {
                        rows.push(usdm_syntax_template_text_row(
                            value,
                            path,
                            &parameter_reference,
                            &parameter_name,
                            &local_dictionaries,
                        ));
                    }
                }
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_syntax_template_text_rows_at(
                    child,
                    &child_path,
                    &local_dictionaries,
                    rows,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_syntax_template_text_rows_at(
                    child,
                    &format!("{path}/{index}"),
                    dictionaries,
                    rows,
                );
            }
        }
        _ => {}
    }
}

fn merged_syntax_template_dictionaries(
    value: &Value,
    inherited: &HashMap<String, SyntaxTemplateDictionary>,
) -> HashMap<String, SyntaxTemplateDictionary> {
    let mut merged = inherited.clone();
    merged.extend(syntax_template_dictionaries(value));
    merged
}

fn collect_usdm_parameter_map_rows_at(
    value: &Value,
    path: &str,
    reference_keys: &HashSet<(String, String, String)>,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if let Some(dictionaries) = object.get("dictionaries").and_then(Value::as_array) {
                for (dictionary_index, dictionary) in dictionaries.iter().enumerate() {
                    let dictionary_path = format!("{path}/dictionaries/{dictionary_index}");
                    let Some(parameter_maps) =
                        dictionary.get("parameterMaps").and_then(Value::as_array)
                    else {
                        continue;
                    };
                    for (map_index, parameter_map) in parameter_maps.iter().enumerate() {
                        rows.push(usdm_parameter_map_row(
                            parameter_map,
                            dictionary,
                            &format!("{dictionary_path}/parameterMaps/{map_index}"),
                            reference_keys,
                        ));
                    }
                }
            }
            for (key, child) in object {
                if key == "dictionaries" {
                    continue;
                }
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_parameter_map_rows_at(child, &child_path, reference_keys, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_parameter_map_rows_at(
                    child,
                    &format!("{path}/{index}"),
                    reference_keys,
                    rows,
                );
            }
        }
        _ => {}
    }
}

fn collect_usdm_condition_rows_at(
    value: &Value,
    path: &str,
    id_types: &HashMap<String, String>,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if let Some(conditions) = object.get("conditions").and_then(Value::as_array) {
                for (index, condition) in conditions.iter().enumerate() {
                    collect_usdm_condition_apply_rows(
                        condition,
                        &format!("{path}/conditions/{index}"),
                        id_types,
                        rows,
                    );
                }
            }
            for (key, child) in object {
                if key == "conditions" {
                    continue;
                }
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_condition_rows_at(child, &child_path, id_types, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_condition_rows_at(child, &format!("{path}/{index}"), id_types, rows);
            }
        }
        _ => {}
    }
}

fn collect_usdm_condition_apply_rows(
    condition: &Value,
    path: &str,
    id_types: &HashMap<String, String>,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(applies_to_ids) = condition.get("appliesToIds").and_then(Value::as_array) else {
        return;
    };
    for applies_to_id in applies_to_ids.iter().filter_map(value_string) {
        rows.push(usdm_condition_row(
            condition,
            path,
            &applies_to_id,
            id_types
                .get(&applies_to_id)
                .map(String::as_str)
                .unwrap_or("[Invalid id]"),
        ));
    }
}

fn usdm_condition_row(
    condition: &Value,
    path: &str,
    applies_to_id: &str,
    applies_to_instance_type: &str,
) -> BTreeMap<String, Value> {
    let allowed = matches!(
        applies_to_instance_type,
        "Procedure"
            | "Activity"
            | "BiomedicalConcept"
            | "BiomedicalConceptCategory"
            | "BiomedicalConceptSurrogate"
    );
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(condition.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(condition.get("id")));
    row.insert("name".to_owned(), json_string(condition.get("name")));
    row.insert(
        "appliesTo id".to_owned(),
        Value::String(applies_to_id.to_owned()),
    );
    row.insert(
        "appliesTo instanceType".to_owned(),
        Value::String(applies_to_instance_type.to_owned()),
    );
    row.insert(
        "condition_applies_to_invalid".to_owned(),
        Value::Bool(!allowed),
    );
    row
}

fn usdm_parameter_map_row(
    parameter_map: &Value,
    dictionary: &Value,
    path: &str,
    reference_keys: &HashSet<(String, String, String)>,
) -> BTreeMap<String, Value> {
    let reference =
        value_string(parameter_map.get("reference").unwrap_or(&Value::Null)).unwrap_or_default();
    let invalid = parameter_map_reference_invalid(&reference, reference_keys);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(parameter_map.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(parameter_map.get("id")));
    row.insert("tag".to_owned(), json_string(parameter_map.get("tag")));
    row.insert("reference".to_owned(), Value::String(reference));
    row.insert(
        "SyntaxTemplateDictionary.id".to_owned(),
        json_string(dictionary.get("id")),
    );
    row.insert(
        "SyntaxTemplateDictionary.name".to_owned(),
        json_string(dictionary.get("name")),
    );
    row.insert(
        "parameter_map_reference_invalid".to_owned(),
        Value::Bool(invalid),
    );
    row
}

fn usdm_syntax_template_text_row(
    object: &Value,
    path: &str,
    parameter_reference: &str,
    parameter_name: &str,
    dictionaries: &HashMap<String, SyntaxTemplateDictionary>,
) -> BTreeMap<String, Value> {
    let dictionary_id = object.get("dictionaryId").and_then(value_string);
    let dictionary = dictionary_id
        .as_ref()
        .and_then(|id| dictionaries.get(id.as_str()));
    let issue = match (dictionary_id.as_ref(), dictionary) {
        (None, _) => "dictionaryId is missing",
        (Some(_), None) => "dictionaryId is invalid",
        (Some(_), Some(dictionary)) if !dictionary.tags.contains(parameter_name) => {
            "Parameter not in dictionary"
        }
        _ => "",
    };

    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(object.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(object.get("id")));
    row.insert("name".to_owned(), json_string(object.get("name")));
    row.insert(
        "Parameter reference".to_owned(),
        Value::String(parameter_reference.to_owned()),
    );
    row.insert(
        "Parameter name".to_owned(),
        Value::String(parameter_name.to_owned()),
    );
    row.insert(
        "dictionaryId".to_owned(),
        dictionary_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "SyntaxTemplateDictionary.name".to_owned(),
        dictionary
            .map(|dictionary| Value::String(dictionary.name.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert("Issue".to_owned(), Value::String(issue.to_owned()));
    row.insert(
        "syntax_template_tag_invalid".to_owned(),
        Value::Bool(!issue.is_empty()),
    );
    row
}

#[derive(Debug, Clone)]
struct SyntaxTemplateDictionary {
    name: String,
    tags: HashSet<String>,
}

fn syntax_template_dictionaries(version: &Value) -> HashMap<String, SyntaxTemplateDictionary> {
    version
        .get("dictionaries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|dictionary| {
            let id = dictionary.get("id").and_then(value_string)?;
            let name = dictionary
                .get("name")
                .and_then(value_string)
                .unwrap_or_default();
            let tags = dictionary
                .get("parameterMaps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|parameter_map| parameter_map.get("tag").and_then(value_string))
                .collect::<HashSet<_>>();
            Some((id, SyntaxTemplateDictionary { name, tags }))
        })
        .collect()
}

fn syntax_template_text_target_entity(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("instanceType")
        .and_then(Value::as_str)
        .is_some_and(|instance_type| {
            matches!(
                instance_type,
                "EligibilityCriterion"
                    | "EligibilityCriterionItem"
                    | "Characteristic"
                    | "Condition"
                    | "Objective"
                    | "Endpoint"
                    | "IntercurrentEvent"
            )
        })
}
