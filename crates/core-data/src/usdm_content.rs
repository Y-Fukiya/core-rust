use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;

use crate::usdm_references::{
    collect_usdm_reference_keys, parameter_map_reference_invalid, usdm_ref_references,
};
use crate::usdm_values::{format_string_list, json_string, string_array, value_string};

pub(crate) fn collect_usdm_narrative_content_item_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let reference_keys = collect_usdm_reference_keys(value);
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(items) = version
            .get("narrativeContentItems")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for (item_index, item) in items.iter().enumerate() {
            let Some(text) = item.get("text").and_then(Value::as_str) else {
                continue;
            };
            for reference in usdm_ref_references(text) {
                rows.push(usdm_narrative_content_item_row(
                    item,
                    &format!("/study/versions/{version_index}/narrativeContentItems/{item_index}"),
                    &reference,
                    &reference_keys,
                ));
            }
        }
    }
}

pub(crate) fn collect_usdm_narrative_content_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let narrative_content_item_ids = collect_usdm_narrative_content_item_ids(value);
    let reference_keys = collect_usdm_reference_keys(value);
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
            let Some(contents) = version.get("contents").and_then(Value::as_array) else {
                continue;
            };
            let content_ids = contents
                .iter()
                .filter_map(|content| content.get("id").and_then(value_string))
                .collect::<HashSet<_>>();
            let display_section_number_counts = display_section_number_counts_for_version(contents);
            for (content_index, content) in contents.iter().enumerate() {
                rows.push(usdm_narrative_content_row(
                    content,
                    &format!(
                        "/study/documentedBy/{document_index}/versions/{version_index}/contents/{content_index}"
                    ),
                    document,
                    version,
                    &content_ids,
                    &narrative_content_item_ids,
                    &reference_keys,
                    &display_section_number_counts,
                ));
            }
        }
    }
}

pub(crate) fn collect_usdm_document_content_reference_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let documented_by = value
        .get("study")
        .and_then(|study| study.get("documentedBy"));
    let document_names = values_as_slice(documented_by.unwrap_or(&Value::Null))
        .into_iter()
        .filter_map(|document| {
            Some((
                document.get("id").and_then(value_string)?,
                document
                    .get("name")
                    .and_then(value_string)
                    .unwrap_or_default(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(amendments) = version.get("amendments").and_then(Value::as_array) else {
            continue;
        };
        for (amendment_index, amendment) in amendments.iter().enumerate() {
            let Some(changes) = amendment.get("changes").and_then(Value::as_array) else {
                continue;
            };
            let invalid_section_keys = document_content_reference_invalid_keys(changes);
            for (change_index, change) in changes.iter().enumerate() {
                let Some(changed_sections) =
                    change.get("changedSections").and_then(Value::as_array)
                else {
                    continue;
                };
                for (section_index, section) in changed_sections.iter().enumerate() {
                    rows.push(usdm_document_content_reference_row(
                        section,
                        amendment,
                        change,
                        &document_names,
                        &invalid_section_keys,
                        &format!(
                            "/study/versions/{version_index}/amendments/{amendment_index}/changes/{change_index}/changedSections/{section_index}"
                        ),
                    ));
                }
            }
        }
    }
}

pub(crate) fn collect_usdm_timeline_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(study_designs) = version.get("studyDesigns").and_then(Value::as_array) else {
            continue;
        };
        for (design_index, design) in study_designs.iter().enumerate() {
            let Some(timelines) = design.get("scheduleTimelines").and_then(Value::as_array) else {
                continue;
            };
            for (timeline_index, timeline) in timelines.iter().enumerate() {
                rows.push(usdm_timeline_row(
                    timeline,
                    version_index,
                    design_index,
                    timeline_index,
                ));
            }
        }
    }
}

fn display_section_number_counts_for_version(contents: &[Value]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for content in contents {
        if !content
            .get("displaySectionNumber")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(section_number) = content.get("sectionNumber").and_then(value_string) else {
            continue;
        };
        if section_number.is_empty() {
            continue;
        }
        *counts.entry(section_number).or_insert(0) += 1;
    }
    counts
}

fn values_as_slice(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(values) => values.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

fn usdm_narrative_content_item_row(
    item: &Value,
    path: &str,
    reference: &str,
    reference_keys: &HashSet<(String, String, String)>,
) -> BTreeMap<String, Value> {
    let invalid = parameter_map_reference_invalid(reference, reference_keys);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(item.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(item.get("id")));
    row.insert("name".to_owned(), json_string(item.get("name")));
    row.insert(
        "Invalid Reference".to_owned(),
        Value::String(reference.to_owned()),
    );
    row.insert(
        "narrative_content_ref_invalid".to_owned(),
        Value::Bool(invalid),
    );
    row
}

fn collect_usdm_narrative_content_item_ids(value: &Value) -> HashSet<String> {
    value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|version| {
            version
                .get("narrativeContentItems")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|item| item.get("id").and_then(value_string))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn usdm_narrative_content_row(
    content: &Value,
    path: &str,
    document: &Value,
    document_version: &Value,
    content_ids: &HashSet<String>,
    narrative_content_item_ids: &HashSet<String>,
    reference_keys: &HashSet<(String, String, String)>,
    display_section_number_counts: &BTreeMap<String, usize>,
) -> BTreeMap<String, Value> {
    let content_item_id = content.get("contentItemId").and_then(value_string);
    let invalid_content_item_id = content_item_id
        .as_ref()
        .is_some_and(|id| !narrative_content_item_ids.contains(id));
    let previous_id = content.get("previousId").and_then(value_string);
    let next_id = content.get("nextId").and_then(value_string);
    let child_ids = string_array(content.get("childIds"));
    let narrative_content_missing_link = child_ids.is_empty()
        && content_item_id
            .as_ref()
            .is_none_or(|content_item_id| content_item_id.is_empty());
    let invalid_previous_id = previous_id
        .as_ref()
        .filter(|id| !content_ids.contains(*id))
        .cloned();
    let invalid_next_id = next_id
        .as_ref()
        .filter(|id| !content_ids.contains(*id))
        .cloned();
    let invalid_child_ids = child_ids
        .iter()
        .filter(|id| !content_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let narrative_content_peer_ref_invalid =
        invalid_previous_id.is_some() || invalid_next_id.is_some() || !invalid_child_ids.is_empty();
    let invalid_references = content
        .get("text")
        .and_then(Value::as_str)
        .into_iter()
        .flat_map(usdm_ref_references)
        .filter(|reference| parameter_map_reference_invalid(reference, reference_keys))
        .collect::<Vec<_>>();
    let display_section_number = content
        .get("displaySectionNumber")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let display_section_title = content
        .get("displaySectionTitle")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let section_number_missing = content
        .get("sectionNumber")
        .and_then(value_string)
        .is_none_or(|value| value.is_empty());
    let display_section_number_duplicate = display_section_number
        && content
            .get("sectionNumber")
            .and_then(value_string)
            .is_some_and(|section_number| {
                display_section_number_counts
                    .get(&section_number)
                    .is_some_and(|count| *count > 1)
            });
    let section_title_missing = content
        .get("sectionTitle")
        .and_then(value_string)
        .is_none_or(|value| value.is_empty());
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(content.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(content.get("id")));
    row.insert("name".to_owned(), json_string(content.get("name")));
    row.insert(
        "contentItemId".to_owned(),
        content_item_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "sectionNumber".to_owned(),
        json_string(content.get("sectionNumber")),
    );
    row.insert(
        "sectionTitle".to_owned(),
        json_string(content.get("sectionTitle")),
    );
    row.insert(
        "displaySectionNumber".to_owned(),
        Value::Bool(display_section_number),
    );
    row.insert(
        "displaySectionTitle".to_owned(),
        Value::Bool(display_section_title),
    );
    row.insert(
        "previousId".to_owned(),
        previous_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "nextId".to_owned(),
        next_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert("childIds".to_owned(), Value::String(child_ids.join("; ")));
    row.insert(
        "Invalid previousId".to_owned(),
        invalid_previous_id
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "Invalid nextId".to_owned(),
        invalid_next_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "Invalid childIds".to_owned(),
        if invalid_child_ids.is_empty() {
            Value::Null
        } else {
            Value::String(invalid_child_ids.join("; "))
        },
    );
    row.insert(
        "StudyDefinitionDocument.id".to_owned(),
        json_string(document.get("id")),
    );
    row.insert(
        "StudyDefinitionDocument.name".to_owned(),
        json_string(document.get("name")),
    );
    row.insert(
        "StudyProtocolDocument.id".to_owned(),
        json_string(document.get("id")),
    );
    row.insert(
        "StudyProtocolDocument.name".to_owned(),
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
        "StudyProtocolDocumentVersion.id".to_owned(),
        json_string(document_version.get("id")),
    );
    row.insert(
        "StudyProtocolDocumentVersion.protocolVersion".to_owned(),
        json_string(
            document_version
                .get("protocolVersion")
                .or_else(|| document_version.get("version")),
        ),
    );
    row.insert(
        "narrative_content_item_id_invalid".to_owned(),
        Value::Bool(invalid_content_item_id),
    );
    row.insert(
        "narrative_content_peer_ref_invalid".to_owned(),
        Value::Bool(narrative_content_peer_ref_invalid),
    );
    row.insert(
        "Invalid Reference".to_owned(),
        Value::String(format_string_list(&invalid_references)),
    );
    row.insert(
        "narrative_content_invalid_usdm_ref".to_owned(),
        Value::Bool(!invalid_references.is_empty()),
    );
    row.insert(
        "narrative_content_display_section_number_missing".to_owned(),
        Value::Bool(display_section_number && section_number_missing),
    );
    row.insert(
        "narrative_content_display_section_number_duplicate".to_owned(),
        Value::Bool(display_section_number_duplicate),
    );
    row.insert(
        "narrative_content_display_section_title_missing".to_owned(),
        Value::Bool(display_section_title && section_title_missing),
    );
    row.insert(
        "narrative_content_missing_link".to_owned(),
        Value::Bool(narrative_content_missing_link),
    );
    row
}

fn document_content_reference_invalid_keys(changes: &[Value]) -> HashSet<(String, String, String)> {
    let mut titles_by_number = HashMap::<(String, String), BTreeSet<String>>::new();
    let mut numbers_by_title = HashMap::<(String, String), BTreeSet<String>>::new();
    for section in changes
        .iter()
        .flat_map(|change| change.get("changedSections").and_then(Value::as_array))
        .flatten()
    {
        let applies_to_id = section
            .get("appliesToId")
            .and_then(value_string)
            .unwrap_or_default();
        let section_number = section
            .get("sectionNumber")
            .and_then(value_string)
            .unwrap_or_default();
        let section_title = section
            .get("sectionTitle")
            .and_then(value_string)
            .unwrap_or_default();
        titles_by_number
            .entry((applies_to_id.clone(), section_number.clone()))
            .or_default()
            .insert(section_title.clone());
        numbers_by_title
            .entry((applies_to_id, section_title))
            .or_default()
            .insert(section_number);
    }

    changes
        .iter()
        .flat_map(|change| change.get("changedSections").and_then(Value::as_array))
        .flatten()
        .filter_map(|section| {
            let applies_to_id = section
                .get("appliesToId")
                .and_then(value_string)
                .unwrap_or_default();
            let section_number = section
                .get("sectionNumber")
                .and_then(value_string)
                .unwrap_or_default();
            let section_title = section
                .get("sectionTitle")
                .and_then(value_string)
                .unwrap_or_default();
            let title_count = titles_by_number
                .get(&(applies_to_id.clone(), section_number.clone()))
                .map_or(0, BTreeSet::len);
            let number_count = numbers_by_title
                .get(&(applies_to_id.clone(), section_title.clone()))
                .map_or(0, BTreeSet::len);
            (title_count != 1 || number_count != 1).then_some((
                applies_to_id,
                section_number,
                section_title,
            ))
        })
        .collect()
}

fn usdm_document_content_reference_row(
    section: &Value,
    amendment: &Value,
    change: &Value,
    document_names: &HashMap<String, String>,
    invalid_section_keys: &HashSet<(String, String, String)>,
    path: &str,
) -> BTreeMap<String, Value> {
    let applies_to_id = section
        .get("appliesToId")
        .and_then(value_string)
        .unwrap_or_default();
    let section_number = section
        .get("sectionNumber")
        .and_then(value_string)
        .unwrap_or_default();
    let section_title = section
        .get("sectionTitle")
        .and_then(value_string)
        .unwrap_or_default();
    let applies_to_name = document_names
        .get(&applies_to_id)
        .cloned()
        .unwrap_or_else(|| "Invalid appliesToId".to_owned());
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(section.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(section.get("id")));
    row.insert(
        "StudyAmendment.id".to_owned(),
        json_string(amendment.get("id")),
    );
    row.insert(
        "StudyAmendment.name".to_owned(),
        json_string(amendment.get("name")),
    );
    row.insert("StudyChange.id".to_owned(), json_string(change.get("id")));
    row.insert(
        "StudyChange.name".to_owned(),
        json_string(change.get("name")),
    );
    row.insert(
        "appliesToId".to_owned(),
        Value::String(format!("{applies_to_id}: {applies_to_name}")),
    );
    row.insert(
        "sectionNumber".to_owned(),
        Value::String(section_number.clone()),
    );
    row.insert(
        "sectionTitle".to_owned(),
        Value::String(section_title.clone()),
    );
    row.insert(
        "document_content_reference_section_one_to_one_invalid".to_owned(),
        Value::Bool(invalid_section_keys.contains(&(applies_to_id, section_number, section_title))),
    );
    row
}

fn usdm_timeline_row(
    timeline: &Value,
    version_index: usize,
    design_index: usize,
    timeline_index: usize,
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/studyDesigns/{design_index}/scheduleTimelines/{timeline_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(timeline.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(timeline.get("id")));
    row.insert("name".to_owned(), json_string(timeline.get("name")));
    row.insert(
        "mainTimeline".to_owned(),
        Value::Bool(
            timeline
                .get("mainTimeline")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );
    row.insert(
        "plannedDuration.present".to_owned(),
        Value::Bool(
            timeline
                .get("plannedDuration")
                .is_some_and(|duration| !duration.is_null()),
        ),
    );
    row
}
