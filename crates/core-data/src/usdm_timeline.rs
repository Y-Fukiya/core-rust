use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::usdm_values::value_string;

pub(crate) fn ordered_usdm_objects_by_previous_next(value: Option<&Value>) -> Vec<String> {
    let objects = value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let by_id = objects
        .iter()
        .filter_map(|object| Some((object.get("id").and_then(value_string)?, object)))
        .collect::<HashMap<_, _>>();
    let mut current_id = objects
        .iter()
        .find(|object| {
            object
                .get("previousId")
                .and_then(value_string)
                .is_none_or(|previous_id| previous_id.is_empty())
        })
        .and_then(|object| object.get("id").and_then(value_string));
    let mut ordered = Vec::new();
    let mut visited = HashSet::new();
    while let Some(id) = current_id {
        if !visited.insert(id.clone()) {
            break;
        }
        let Some(object) = by_id.get(&id) else {
            break;
        };
        if let Some(label) = format_usdm_id_name(object) {
            ordered.push(label);
        }
        current_id = object.get("nextId").and_then(value_string);
    }
    ordered
}

pub(crate) fn timeline_usdm_object_ref_order(
    timeline: &Value,
    objects: Option<&Value>,
    reference_field: &str,
) -> Vec<String> {
    let object_by_id = objects
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|object| Some((object.get("id").and_then(value_string)?, object)))
        .collect::<HashMap<_, _>>();
    let mut ordered = Vec::new();
    let mut previous_ref: Option<String> = None;
    for instance in timeline
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if instance.get("instanceType").and_then(Value::as_str) != Some("ScheduledActivityInstance")
        {
            continue;
        }
        let Some(reference_id) = instance.get(reference_field).and_then(value_string) else {
            continue;
        };
        if previous_ref.as_ref() == Some(&reference_id) {
            continue;
        }
        previous_ref = Some(reference_id.clone());
        if let Some(object) = object_by_id.get(&reference_id) {
            if let Some(label) = format_usdm_id_name(object) {
                ordered.push(label);
            }
        }
    }
    ordered
}

pub(crate) fn format_usdm_object_order(values: &[String]) -> String {
    format!("[ {} ]", values.join(" > "))
}

pub(crate) fn format_timeline_names(timelines: &[&Value]) -> String {
    if timelines.is_empty() {
        return "null".to_owned();
    }

    format!(
        "[{}]",
        timelines
            .iter()
            .map(|timeline| {
                let id =
                    value_string(timeline.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
                let name =
                    value_string(timeline.get("name").unwrap_or(&Value::Null)).unwrap_or_default();
                if name.is_empty() {
                    id
                } else {
                    format!("{id} [{name}]")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(crate) fn format_usdm_id_name(value: &Value) -> Option<String> {
    let id = value.get("id").and_then(value_string)?;
    let name = value.get("name").and_then(value_string).unwrap_or_default();
    Some(format!("{id}: {name}"))
}
