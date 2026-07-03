use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::duplicate_strings;
use crate::usdm_timeline::{
    format_timeline_names, format_usdm_object_order, ordered_usdm_objects_by_previous_next,
    timeline_usdm_object_ref_order,
};
use crate::usdm_values::{
    format_code, format_string_list, json_string, string_array, value_string,
};

pub(crate) fn collect_usdm_design_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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
            rows.push(usdm_design_row(
                design,
                version,
                version_index,
                design_index,
            ));
        }
    }
}

pub(crate) fn collect_usdm_design_list_duplicate_rows(
    value: &Value,
    characteristic_rows: &mut Vec<BTreeMap<String, Value>>,
    sub_type_rows: &mut Vec<BTreeMap<String, Value>>,
    therapeutic_area_rows: &mut Vec<BTreeMap<String, Value>>,
) {
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
            let path = format!("/study/versions/{version_index}/studyDesigns/{design_index}");
            collect_duplicate_code_rows(
                design,
                &path,
                "characteristics",
                "characteristics",
                characteristic_rows,
            );
            collect_duplicate_code_rows(design, &path, "subTypes", "subTypes", sub_type_rows);
            collect_duplicate_therapeutic_area_rows(design, &path, therapeutic_area_rows);
        }
    }
}

pub(crate) fn collect_usdm_interventional_design_rows(
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
        let Some(study_designs) = version.get("studyDesigns").and_then(Value::as_array) else {
            continue;
        };
        for (design_index, design) in study_designs.iter().enumerate() {
            rows.push(usdm_interventional_design_row(
                design,
                version,
                version_index,
                design_index,
            ));
        }
    }
}

fn collect_duplicate_code_rows(
    design: &Value,
    path: &str,
    source_key: &str,
    output_key: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(values) = design.get(source_key).and_then(Value::as_array) else {
        return;
    };
    let mut by_code: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for value in values {
        if let Some(code) = value.get("code").and_then(value_string) {
            by_code.entry(code).or_default().push(value);
        }
    }
    for duplicates in by_code.values().filter(|values| values.len() > 1) {
        let mut row = BTreeMap::new();
        row.insert("path".to_owned(), Value::String(path.to_owned()));
        row.insert("name".to_owned(), json_string(design.get("name")));
        row.insert(
            "study_design_duplicate_list_row".to_owned(),
            Value::Bool(true),
        );
        row.insert(
            output_key.to_owned(),
            Value::String(format_code_object_list(duplicates)),
        );
        rows.push(row);
    }
}

fn collect_duplicate_therapeutic_area_rows(
    design: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(values) = design.get("therapeuticAreas").and_then(Value::as_array) else {
        return;
    };
    let mut by_key: BTreeMap<(String, String, String), Vec<&Value>> = BTreeMap::new();
    for value in values {
        let key = (
            value
                .get("codeSystem")
                .and_then(value_string)
                .unwrap_or_default(),
            value
                .get("codeSystemVersion")
                .and_then(value_string)
                .unwrap_or_default(),
            value.get("code").and_then(value_string).unwrap_or_default(),
        );
        by_key.entry(key).or_default().push(value);
    }
    for ((code_system, code_system_version, _code), duplicates) in
        by_key.into_iter().filter(|(_, values)| values.len() > 1)
    {
        let mut row = BTreeMap::new();
        row.insert("path".to_owned(), Value::String(path.to_owned()));
        row.insert("name".to_owned(), json_string(design.get("name")));
        row.insert(
            "study_design_duplicate_list_row".to_owned(),
            Value::Bool(true),
        );
        row.insert(
            "therapeuticAreas.codeSystem".to_owned(),
            Value::String(code_system),
        );
        row.insert(
            "therapeuticAreas.codeSystemVersion".to_owned(),
            Value::String(code_system_version),
        );
        row.insert(
            "therapeuticAreas".to_owned(),
            Value::String(format_code_object_list(&duplicates)),
        );
        rows.push(row);
    }
}

fn usdm_design_row(
    design: &Value,
    version: &Value,
    version_index: usize,
    design_index: usize,
) -> BTreeMap<String, Value> {
    let study_type = design.get("studyType").or_else(|| version.get("studyType"));
    let study_type_code = study_type
        .and_then(|code| code.get("code"))
        .and_then(value_string);
    let study_phase = design
        .get("studyPhase")
        .or_else(|| version.get("studyPhase"));
    let study_phase_code = study_phase
        .and_then(|phase| phase.get("standardCode"))
        .and_then(|code| code.get("code"))
        .and_then(value_string);
    let document_version_ids = string_array(design.get("documentVersionIds"));
    let duplicate_document_version_ids = duplicate_strings(&document_version_ids);
    let characteristics = design
        .get("characteristics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let characteristic_codes = characteristics
        .iter()
        .filter_map(|code| code.get("code").and_then(value_string))
        .collect::<Vec<_>>();
    let intent_types = design
        .get("intentTypes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_intent_types = duplicate_code_values(&intent_types);
    let duplicate_intent_type_group_count = duplicate_code_group_count(&intent_types);
    let intervention_model = design.get("interventionModel");
    let intervention_model_code = intervention_model
        .and_then(|model| model.get("code"))
        .and_then(value_string);
    let study_intervention_count = design
        .get("studyInterventions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let referenced_interventions = referenced_study_intervention_count(design, version);
    let primary_endpoints = primary_endpoint_count(design);
    let main_timelines = design
        .get("scheduleTimelines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|timeline| {
            timeline
                .get("mainTimeline")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let encounter_previous_next_order =
        ordered_usdm_objects_by_previous_next(design.get("encounters"));
    let encounter_timeline_order = main_timelines
        .first()
        .map(|timeline| {
            timeline_usdm_object_ref_order(timeline, design.get("encounters"), "encounterId")
        })
        .unwrap_or_default();
    let epoch_previous_next_order = ordered_usdm_objects_by_previous_next(design.get("epochs"));
    let epoch_timeline_order = main_timelines
        .first()
        .map(|timeline| timeline_usdm_object_ref_order(timeline, design.get("epochs"), "epochId"))
        .unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/studyDesigns/{design_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(design.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(design.get("id")));
    row.insert("name".to_owned(), json_string(design.get("name")));
    row.insert(
        "studyType.code".to_owned(),
        study_type_code
            .as_ref()
            .map(|code| Value::String(code.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "studyType.decode".to_owned(),
        json_string(study_type.and_then(|code| code.get("decode"))),
    );
    row.insert(
        "studyType".to_owned(),
        study_type
            .map(|code| format_code(Some(code)))
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "studyPhase.standardCode.code".to_owned(),
        study_phase_code
            .as_ref()
            .map(|code| Value::String(code.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "studyPhase".to_owned(),
        study_phase
            .and_then(|phase| phase.get("standardCode"))
            .map(|code| format_code(Some(code)))
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "Duplicate documentVersionIds".to_owned(),
        Value::String(format_string_list(&duplicate_document_version_ids)),
    );
    row.insert(
        "characteristics".to_owned(),
        Value::String(format_filtered_code_values(
            &characteristics,
            &["C217004", "C217005"],
        )),
    );
    row.insert(
        "intentTypes".to_owned(),
        Value::String(format_duplicate_code_values(&duplicate_intent_types)),
    );
    row.insert(
        "intentTypes.duplicate_group_count".to_owned(),
        Value::Number(serde_json::Number::from(duplicate_intent_type_group_count)),
    );
    row.insert(
        "study_design_duplicate_document_version_ids".to_owned(),
        Value::Bool(!duplicate_document_version_ids.is_empty()),
    );
    row.insert(
        "study_design_duplicate_list_row".to_owned(),
        Value::Bool(false),
    );
    row.insert(
        "observational_design_wrong_class".to_owned(),
        Value::Bool(
            matches!(study_type_code.as_deref(), Some("C16084" | "C129000"))
                && design.get("instanceType").and_then(Value::as_str)
                    != Some("ObservationalStudyDesign"),
        ),
    );
    row.insert(
        "observational_design_wrong_phase".to_owned(),
        Value::Bool(
            matches!(study_type_code.as_deref(), Some("C16084" | "C129000"))
                && design.get("instanceType").and_then(Value::as_str)
                    == Some("ObservationalStudyDesign")
                && study_phase_code.as_deref() != Some("C48660"),
        ),
    );
    row.insert(
        "study_design_single_and_multi_centre".to_owned(),
        Value::Bool(
            characteristic_codes.iter().any(|code| code == "C217004")
                && characteristic_codes.iter().any(|code| code == "C217005"),
        ),
    );
    row.insert(
        "interventional_design_wrong_class".to_owned(),
        Value::Bool(
            study_type_code.as_deref() == Some("C98388")
                && design.get("instanceType").and_then(Value::as_str)
                    != Some("InterventionalStudyDesign"),
        ),
    );
    row.insert(
        "study_design_single_and_multiple_countries".to_owned(),
        Value::Bool(
            characteristic_codes.iter().any(|code| code == "C217006")
                && characteristic_codes.iter().any(|code| code == "C217007"),
        ),
    );
    row.insert(
        "study_design_randomization_characteristic_conflict".to_owned(),
        Value::Bool(
            characteristic_codes
                .iter()
                .filter(|code| matches!(code.as_str(), "C46079" | "C25689" | "C147145"))
                .count()
                > 1,
        ),
    );
    row.insert(
        "study_design_duplicate_intent_types".to_owned(),
        Value::Bool(!duplicate_intent_types.is_empty()),
    );
    row.insert(
        "study_design_encounter_timeline_order_mismatch".to_owned(),
        Value::Bool(
            !encounter_previous_next_order.is_empty()
                && !encounter_timeline_order.is_empty()
                && encounter_previous_next_order != encounter_timeline_order,
        ),
    );
    row.insert(
        "study_design_epoch_timeline_order_mismatch".to_owned(),
        Value::Bool(
            !epoch_previous_next_order.is_empty()
                && !epoch_timeline_order.is_empty()
                && epoch_previous_next_order != epoch_timeline_order,
        ),
    );
    row.insert(
        "interventionModel.code".to_owned(),
        intervention_model_code
            .as_ref()
            .map(|code| Value::String(code.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "interventionModel.decode".to_owned(),
        json_string(intervention_model.and_then(|model| model.get("decode"))),
    );
    row.insert(
        "# Study Interventions".to_owned(),
        Value::Number(serde_json::Number::from(study_intervention_count)),
    );
    row.insert(
        "study_design_intervention_model_count_inconsistent".to_owned(),
        Value::Bool(intervention_model_code.as_deref().is_some_and(|code| {
            (code == "C82640" && study_intervention_count != 1)
                || (code != "C82640" && study_intervention_count <= 1)
        })),
    );
    row.insert(
        "model.code".to_owned(),
        json_string(design.get("model").and_then(|code| code.get("code"))),
    );
    row.insert(
        "model.decode".to_owned(),
        json_string(design.get("model").and_then(|code| code.get("decode"))),
    );
    row.insert(
        "# Referenced Study Interventions".to_owned(),
        Value::Number(serde_json::Number::from(referenced_interventions)),
    );
    row.insert(
        "# Primary endpoints".to_owned(),
        Value::Number(serde_json::Number::from(primary_endpoints)),
    );
    row.insert(
        "# Main timelines".to_owned(),
        Value::Number(serde_json::Number::from(main_timelines.len())),
    );
    row.insert(
        "Main timelines".to_owned(),
        Value::String(format_timeline_names(&main_timelines)),
    );
    row.insert(
        "ScheduleTimeline.id".to_owned(),
        main_timelines
            .first()
            .and_then(|timeline| timeline.get("id"))
            .map(|value| json_string(Some(value)))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "ScheduleTimeline.name".to_owned(),
        main_timelines
            .first()
            .and_then(|timeline| timeline.get("name"))
            .map(|value| json_string(Some(value)))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "ScheduleTimeline.mainTimeline".to_owned(),
        Value::Bool(!main_timelines.is_empty()),
    );
    row.insert(
        "Encounter order by previous/next".to_owned(),
        Value::String(format_usdm_object_order(&encounter_previous_next_order)),
    );
    row.insert(
        "Encounter order by timeline refs".to_owned(),
        Value::String(format_usdm_object_order(&encounter_timeline_order)),
    );
    row.insert(
        "Epoch order by previous/next".to_owned(),
        Value::String(format_usdm_object_order(&epoch_previous_next_order)),
    );
    row.insert(
        "Epoch order by timeline refs".to_owned(),
        Value::String(format_usdm_object_order(&epoch_timeline_order)),
    );
    row
}

fn usdm_interventional_design_row(
    design: &Value,
    version: &Value,
    version_index: usize,
    design_index: usize,
) -> BTreeMap<String, Value> {
    let mut row = usdm_design_row(design, version, version_index, design_index);
    let design_id = value_string(design.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
    let version_id = value_string(version.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
    let applicable_roles = applicable_masking_roles(version, &version_id, &design_id);
    let masked_roles = applicable_roles
        .iter()
        .filter(|role| role.is_masked)
        .collect::<Vec<_>>();
    let blinding_code = design
        .get("blindingSchema")
        .and_then(|schema| schema.get("standardCode"))
        .and_then(|code| code.get("code"));
    let blinding_decode = design
        .get("blindingSchema")
        .and_then(|schema| schema.get("standardCode"))
        .and_then(|code| code.get("decode"));
    let blinding_code_string = blinding_code.and_then(value_string).unwrap_or_default();
    let has_blinding_schema = !blinding_code_string.is_empty();
    let requires_masked_role =
        has_blinding_schema && !matches!(blinding_code_string.as_str(), "C49659" | "C15228");
    row.insert(
        "instanceType".to_owned(),
        Value::String("InterventionalStudyDesign".to_owned()),
    );
    row.insert("blindingSchema.code".to_owned(), json_string(blinding_code));
    row.insert(
        "blindingSchema.decode".to_owned(),
        json_string(blinding_decode),
    );
    row.insert(
        "# Masked Roles".to_owned(),
        Value::Number(serde_json::Number::from(masked_roles.len())),
    );
    row.insert(
        "Applicable Roles".to_owned(),
        Value::String(format_applicable_roles(&applicable_roles)),
    );
    row.insert(
        "blinding_schema_missing_masked_role".to_owned(),
        Value::Bool(requires_masked_role && masked_roles.is_empty()),
    );
    row.insert(
        "# Referenced Study Interventions".to_owned(),
        Value::Number(serde_json::Number::from(
            referenced_study_intervention_id_count(design, version),
        )),
    );
    row
}

fn format_code_object_list(values: &[&Value]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| {
                let id = value.get("id").and_then(value_string).unwrap_or_default();
                let decode = value
                    .get("decode")
                    .and_then(value_string)
                    .unwrap_or_default();
                let code = value.get("code").and_then(value_string).unwrap_or_default();
                format!("{id}: {decode} ({code})")
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn format_filtered_code_values(values: &[Value], codes: &[&str]) -> String {
    let filtered = values
        .iter()
        .filter(|value| {
            value
                .get("code")
                .and_then(value_string)
                .is_some_and(|code| codes.iter().any(|expected| *expected == code))
        })
        .collect::<Vec<_>>();
    format_code_object_list(&filtered)
}

fn duplicate_code_values(values: &[Value]) -> Vec<&Value> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        if let Some(code) = value.get("code").and_then(value_string) {
            *counts.entry(code).or_insert(0) += 1;
        }
    }
    values
        .iter()
        .filter(|value| {
            value
                .get("code")
                .and_then(value_string)
                .is_some_and(|code| counts.get(&code).is_some_and(|count| *count > 1))
        })
        .collect()
}

fn duplicate_code_group_count(values: &[Value]) -> usize {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        if let Some(code) = value.get("code").and_then(value_string) {
            *counts.entry(code).or_insert(0) += 1;
        }
    }
    counts.values().filter(|count| **count > 1).count()
}

fn format_duplicate_code_values(values: &[&Value]) -> String {
    if values.is_empty() {
        String::new()
    } else {
        format_code_object_list(values)
    }
}

fn referenced_study_intervention_count(design: &Value, version: &Value) -> usize {
    let mut valid_ids = string_array(design.get("studyInterventionIds"))
        .into_iter()
        .filter(|id| {
            version
                .get("studyInterventions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|intervention| {
                    value_string(intervention.get("id").unwrap_or(&Value::Null)).as_ref()
                        == Some(id)
                })
        })
        .collect::<Vec<_>>();
    valid_ids.extend(
        design
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|intervention| {
                value_string(intervention.get("id").unwrap_or(&Value::Null))
            }),
    );

    design
        .get("activities")
        .into_iter()
        .flat_map(activity_defined_procedures)
        .filter(|procedure| {
            procedure
                .get("studyInterventionId")
                .and_then(value_string)
                .is_some_and(|id| valid_ids.iter().any(|valid| valid == &id))
        })
        .count()
}

#[derive(Debug, Clone)]
struct ApplicableMaskingRole {
    id: String,
    code: String,
    is_masked: bool,
}

fn applicable_masking_roles(
    version: &Value,
    version_id: &str,
    design_id: &str,
) -> Vec<ApplicableMaskingRole> {
    version
        .get("roles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|role| {
            let applies_to_ids = string_array(role.get("appliesToIds"));
            applies_to_ids
                .iter()
                .any(|id| id == version_id || id == design_id)
        })
        .map(|role| ApplicableMaskingRole {
            id: value_string(role.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
            code: role
                .get("code")
                .and_then(|code| code.get("decode"))
                .and_then(value_string)
                .unwrap_or_default(),
            is_masked: role
                .get("masking")
                .and_then(|masking| masking.get("isMasked"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

fn format_applicable_roles(roles: &[ApplicableMaskingRole]) -> String {
    roles
        .iter()
        .map(|role| {
            format!(
                "{}[{},{}]",
                role.code,
                role.id,
                if role.is_masked {
                    "Masked"
                } else {
                    "Not Masked"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn referenced_study_intervention_id_count(design: &Value, version: &Value) -> usize {
    let valid_version_ids = version
        .get("studyInterventions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|intervention| value_string(intervention.get("id").unwrap_or(&Value::Null)))
        .collect::<HashSet<_>>();

    string_array(design.get("studyInterventionIds"))
        .into_iter()
        .filter(|id| valid_version_ids.contains(id))
        .collect::<HashSet<_>>()
        .len()
}

fn activity_defined_procedures(activity_value: &Value) -> Vec<&Value> {
    if let Some(activities) = activity_value.as_array() {
        return activities
            .iter()
            .flat_map(activity_defined_procedures)
            .collect();
    }

    activity_value
        .get("definedProcedures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn primary_endpoint_count(design: &Value) -> usize {
    design
        .get("objectives")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|objective| {
            objective
                .get("endpoints")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|endpoint| {
            endpoint
                .get("level")
                .and_then(|level| level.get("code"))
                .and_then(value_string)
                .as_deref()
                == Some("C94496")
        })
        .count()
}
