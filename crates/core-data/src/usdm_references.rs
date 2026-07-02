use std::collections::{HashMap, HashSet};

use serde_json::Value;

pub(super) fn usdm_tag_references(text: &str) -> Vec<(String, String)> {
    let mut references = Vec::new();
    let mut search_from = 0;
    while let Some(relative_start) = text[search_from..].find("<usdm:tag") {
        let start = search_from + relative_start;
        let Some(relative_end) = text[start..].find('>') else {
            break;
        };
        let mut end = start + relative_end + 1;
        if !text[start..end].ends_with("/>") && text[end..].starts_with("</usdm:tag>") {
            end += "</usdm:tag>".len();
        }
        let reference = text[start..end].to_owned();
        if let Some(name) = parse_xml_like_attribute(&reference, "name") {
            references.push((reference, name));
        }
        search_from = end;
    }
    references
}

pub(super) fn usdm_ref_references(text: &str) -> Vec<String> {
    let mut references = Vec::new();
    let mut search_from = 0;
    while let Some(relative_start) = text[search_from..].find("<usdm:ref") {
        let start = search_from + relative_start;
        let Some(relative_end) = text[start..].find('>') else {
            break;
        };
        let mut end = start + relative_end + 1;
        if !text[start..end].ends_with("/>") && text[end..].starts_with("</usdm:ref>") {
            end += "</usdm:ref>".len();
        }
        references.push(text[start..end].to_owned());
        search_from = end;
    }
    references
}

pub(super) fn parameter_map_reference_invalid(
    reference: &str,
    reference_keys: &HashSet<(String, String, String)>,
) -> bool {
    if !reference.contains("usdm:ref") {
        return false;
    }
    let Some(attributes) = parse_usdm_ref_attributes(reference) else {
        return true;
    };
    let Some(klass) = attributes.get("klass") else {
        return true;
    };
    let Some(id) = attributes.get("id") else {
        return true;
    };
    let Some(attribute) = attributes.get("attribute") else {
        return true;
    };
    !reference_keys.contains(&(klass.clone(), id.clone(), attribute.clone()))
}

pub(super) fn collect_usdm_id_instance_types(value: &Value) -> HashMap<String, String> {
    let mut values = HashMap::new();
    collect_usdm_id_instance_types_at(value, &mut values);
    values
}

pub(super) fn collect_usdm_reference_keys(value: &Value) -> HashSet<(String, String, String)> {
    let mut values = HashSet::new();
    collect_usdm_reference_keys_at(value, &mut values);
    values
}

fn collect_usdm_reference_keys_at(value: &Value, values: &mut HashSet<(String, String, String)>) {
    match value {
        Value::Object(object) => {
            if let (Some(id), Some(instance_type)) = (
                object.get("id").and_then(value_string),
                object.get("instanceType").and_then(value_string),
            ) {
                for key in object.keys() {
                    values.insert((instance_type.clone(), id.clone(), key.clone()));
                }
            }
            for child in object.values() {
                collect_usdm_reference_keys_at(child, values);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_usdm_reference_keys_at(child, values);
            }
        }
        _ => {}
    }
}

fn collect_usdm_id_instance_types_at(value: &Value, values: &mut HashMap<String, String>) {
    match value {
        Value::Object(object) => {
            if let (Some(id), Some(instance_type)) = (
                object.get("id").and_then(value_string),
                object.get("instanceType").and_then(value_string),
            ) {
                values.insert(id, instance_type);
            }
            for child in object.values() {
                collect_usdm_id_instance_types_at(child, values);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_usdm_id_instance_types_at(child, values);
            }
        }
        _ => {}
    }
}

fn parse_usdm_ref_attributes(reference: &str) -> Option<HashMap<String, String>> {
    let start = reference.find("<usdm:ref")?;
    let rest = &reference[start + "<usdm:ref".len()..];
    let end = rest.find('>')?;
    let tag = &rest[..end];
    let mut attributes = HashMap::new();
    for key in ["klass", "id", "attribute"] {
        if let Some(value) = parse_xml_like_attribute(tag, key) {
            attributes.insert(key.to_owned(), value);
        }
    }
    Some(attributes)
}

fn parse_xml_like_attribute(tag: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}
