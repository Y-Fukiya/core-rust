use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::usdm_common::named_usdm_object_name;
use crate::usdm_population_columns::insert_planned_sex_columns;
use crate::usdm_values::{format_string_list, json_string, value_string};

pub(crate) fn collect_usdm_cohort_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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
            let Some(cohorts) = design
                .get("population")
                .and_then(|population| population.get("cohorts"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for (cohort_index, cohort) in cohorts.iter().enumerate() {
                rows.push(usdm_cohort_row(
                    cohort,
                    design,
                    design.get("population").unwrap_or(&Value::Null),
                    version_index,
                    design_index,
                    cohort_index,
                ));
            }
        }
    }
}

pub(crate) fn collect_usdm_study_cell_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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
            let arms = design
                .get("arms")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let epochs = design
                .get("epochs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let Some(study_cells) = design.get("studyCells").and_then(Value::as_array) else {
                continue;
            };
            let mut design_rows = study_cells
                .iter()
                .enumerate()
                .map(|(cell_index, cell)| {
                    usdm_study_cell_row(
                        cell,
                        design,
                        &arms,
                        &epochs,
                        version_index,
                        design_index,
                        cell_index,
                    )
                })
                .collect::<Vec<_>>();
            apply_study_cell_duplicate_flags(&mut design_rows);
            rows.extend(design_rows);
        }
    }
}

fn usdm_cohort_row(
    cohort: &Value,
    design: &Value,
    population: &Value,
    version_index: usize,
    design_index: usize,
    cohort_index: usize,
) -> BTreeMap<String, Value> {
    let indication_ids = collect_direct_ids(design.get("indications"));
    let invalid_indications = string_array(cohort.get("indicationIds"))
        .into_iter()
        .filter(|id| !indication_ids.contains(id))
        .collect::<Vec<_>>();
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/studyDesigns/{design_index}/population/cohorts/{cohort_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(cohort.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(cohort.get("id")));
    row.insert("name".to_owned(), json_string(cohort.get("name")));
    insert_study_design_context(&mut row, design);
    row.insert(
        "StudyDesign.indications.id".to_owned(),
        Value::String(format_string_list(
            &indication_ids.iter().cloned().collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "StudyDesignPopulation.id".to_owned(),
        json_string(population.get("id")),
    );
    row.insert(
        "StudyDesignPopulation.name".to_owned(),
        json_string(population.get("name")),
    );
    row.insert(
        "Invalid indicationIds".to_owned(),
        Value::String(format_string_list(&invalid_indications)),
    );
    row.insert(
        "study_cohort_invalid_indication".to_owned(),
        Value::Bool(!invalid_indications.is_empty()),
    );
    insert_planned_sex_columns(&mut row, cohort.get("plannedSex"));
    row
}

fn usdm_study_cell_row(
    cell: &Value,
    design: &Value,
    arms: &[Value],
    epochs: &[Value],
    version_index: usize,
    design_index: usize,
    cell_index: usize,
) -> BTreeMap<String, Value> {
    let arm_id = value_string(cell.get("armId").unwrap_or(&Value::Null)).unwrap_or_default();
    let epoch_id = value_string(cell.get("epochId").unwrap_or(&Value::Null)).unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/studyDesigns/{design_index}/studyCells/{cell_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(cell.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(cell.get("id")));
    row.insert("armId".to_owned(), Value::String(arm_id.clone()));
    row.insert("epochId".to_owned(), Value::String(epoch_id.clone()));
    row.insert("StudyDesign.id".to_owned(), json_string(design.get("id")));
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(design.get("name")),
    );
    row.insert(
        "StudyArm.name".to_owned(),
        named_usdm_object_name(arms, &arm_id)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "StudyEpoch.name".to_owned(),
        named_usdm_object_name(epochs, &epoch_id)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row
}

fn apply_study_cell_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut counts: HashMap<(String, String, String), usize> = HashMap::new();
    for row in rows.iter() {
        let design_id = row
            .get("StudyDesign.id")
            .and_then(value_string)
            .unwrap_or_default();
        let arm_id = row.get("armId").and_then(value_string).unwrap_or_default();
        let epoch_id = row
            .get("epochId")
            .and_then(value_string)
            .unwrap_or_default();
        *counts.entry((design_id, arm_id, epoch_id)).or_insert(0) += 1;
    }
    for row in rows.iter_mut() {
        let design_id = row
            .get("StudyDesign.id")
            .and_then(value_string)
            .unwrap_or_default();
        let arm_id = row.get("armId").and_then(value_string).unwrap_or_default();
        let epoch_id = row
            .get("epochId")
            .and_then(value_string)
            .unwrap_or_default();
        let duplicate = counts
            .get(&(design_id, arm_id, epoch_id))
            .is_some_and(|count| *count > 1);
        row.insert(
            "study_cell_arm_epoch_duplicate".to_owned(),
            Value::Bool(duplicate),
        );
    }
}

fn insert_study_design_context(row: &mut BTreeMap<String, Value>, design: &Value) {
    row.insert("StudyDesign.id".to_owned(), json_string(design.get("id")));
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(design.get("name")),
    );
}

fn collect_direct_ids(value: Option<&Value>) -> HashSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("id").and_then(value_string))
        .collect()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(value_string)
        .collect()
}
