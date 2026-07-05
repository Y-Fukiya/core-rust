use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::dataset_paths::extension;
use crate::usdm_abbreviations::collect_usdm_abbreviation_rows;
use crate::usdm_collectors::{
    collect_usdm_address_rows, collect_usdm_duration_rows, collect_usdm_person_name_rows,
    collect_usdm_range_rows,
};
use crate::usdm_common::{
    collect_direct_ids, collect_managed_site_ids, collect_nested_ids, duplicate_strings,
    named_usdm_object_name,
};
use crate::usdm_content::{
    collect_usdm_document_content_reference_rows, collect_usdm_narrative_content_item_rows,
    collect_usdm_narrative_content_rows, collect_usdm_scheduled_instance_rows,
    collect_usdm_timeline_rows,
};
use crate::usdm_data_dir_datasets::{
    push_usdm_dataset, push_usdm_dataset_even_when_empty, push_usdm_identifier_datasets,
};
use crate::usdm_design::{
    collect_usdm_design_list_duplicate_rows, collect_usdm_design_rows,
    collect_usdm_interventional_design_rows,
};
use crate::usdm_geography::{
    collect_usdm_geographic_scope_rows, collect_usdm_governance_date_rows,
};
use crate::usdm_identifiers::collect_usdm_identifier_rows;
use crate::usdm_json_schema::collect_usdm_json_schema_issue_rows;
use crate::usdm_objects::{apply_usdm_object_duplicate_flags, collect_usdm_object_rows};
use crate::usdm_population_columns::{insert_planned_sex_columns, insert_quantity_columns};
use crate::usdm_product::{
    collect_usdm_administrable_product_rows, collect_usdm_administration_rows,
    collect_usdm_amendment_reason_rows, collect_usdm_product_organization_role_rows,
    collect_usdm_strength_rows,
};
use crate::usdm_study_structure::{collect_usdm_cohort_rows, collect_usdm_study_cell_rows};
use crate::usdm_text_templates::{
    collect_usdm_condition_rows, collect_usdm_parameter_map_rows,
    collect_usdm_syntax_template_text_rows,
};
use crate::usdm_timeline::format_usdm_id_name;
use crate::usdm_values::{
    format_code, format_semicolon_list, format_string_list, json_string, string_array,
    value_exists, value_string,
};
use crate::{DataError, LoadDataResult, Result};

pub(crate) fn load_open_rules_json_data_dir(data_dir: &Path) -> Result<LoadDataResult> {
    let mut population_rows = Vec::new();
    let mut role_rows = Vec::new();
    let mut role_blinding_rows = Vec::new();
    let mut design_rows = Vec::new();
    let mut interventional_design_rows = Vec::new();
    let mut design_characteristic_rows = Vec::new();
    let mut design_sub_type_rows = Vec::new();
    let mut design_therapeutic_area_rows = Vec::new();
    let mut version_rows = Vec::new();
    let mut activity_rows = Vec::new();
    let mut duration_rows = Vec::new();
    let mut range_rows = Vec::new();
    let mut person_name_rows = Vec::new();
    let mut address_rows = Vec::new();
    let mut administrable_product_rows = Vec::new();
    let mut administration_rows = Vec::new();
    let mut strength_rows = Vec::new();
    let mut amendment_reason_rows = Vec::new();
    let mut product_organization_role_rows = Vec::new();
    let mut biomedical_concept_rows = Vec::new();
    let mut procedure_rows = Vec::new();
    let mut subject_enrollment_rows = Vec::new();
    let mut document_version_rows = Vec::new();
    let mut document_content_reference_rows = Vec::new();
    let mut substance_rows = Vec::new();
    let mut eligibility_criterion_rows = Vec::new();
    let mut eligibility_criterion_item_rows = Vec::new();
    let mut biospecimen_retention_rows = Vec::new();
    let mut study_element_rows = Vec::new();
    let mut study_arm_rows = Vec::new();
    let mut cohort_rows = Vec::new();
    let mut study_cell_rows = Vec::new();
    let mut condition_rows = Vec::new();
    let mut parameter_map_rows = Vec::new();
    let mut syntax_template_text_rows = Vec::new();
    let mut narrative_content_rows = Vec::new();
    let mut narrative_content_item_rows = Vec::new();
    let mut abbreviation_rows = Vec::new();
    let mut object_rows = Vec::new();
    let mut geographic_scope_rows = Vec::new();
    let mut governance_date_rows = Vec::new();
    let mut timeline_rows = Vec::new();
    let mut scheduled_instance_rows = Vec::new();
    let mut identifier_rows = Vec::new();
    let mut json_schema_issue_rows = Vec::new();
    let entries = fs::read_dir(data_dir)
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?;

    for entry in entries {
        let path = entry.path();
        if !path.is_file() || extension(&path).as_deref() != Some("json") {
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|source| DataError::Io {
            path: path.clone(),
            source,
        })?;
        let value =
            serde_json::from_str::<Value>(&source).map_err(|source| DataError::JsonParse {
                path: path.clone(),
                source,
            })?;
        collect_usdm_population_rows(&value, &mut population_rows);
        collect_usdm_role_rows(&value, &mut role_rows);
        collect_usdm_role_blinding_rows(&value, &mut role_blinding_rows);
        collect_usdm_design_rows(&value, &mut design_rows);
        collect_usdm_design_list_duplicate_rows(
            &value,
            &mut design_characteristic_rows,
            &mut design_sub_type_rows,
            &mut design_therapeutic_area_rows,
        );
        collect_usdm_interventional_design_rows(&value, &mut interventional_design_rows);
        collect_usdm_version_rows(&value, &mut version_rows);
        collect_usdm_activity_rows(&value, &mut activity_rows);
        collect_usdm_duration_rows(&value, &mut duration_rows);
        collect_usdm_range_rows(&value, &mut range_rows);
        collect_usdm_person_name_rows(&value, &mut person_name_rows);
        collect_usdm_address_rows(&value, &mut address_rows);
        collect_usdm_administrable_product_rows(&value, &mut administrable_product_rows);
        collect_usdm_administration_rows(&value, &mut administration_rows);
        collect_usdm_strength_rows(&value, &mut strength_rows);
        collect_usdm_amendment_reason_rows(&value, &mut amendment_reason_rows);
        collect_usdm_product_organization_role_rows(&value, &mut product_organization_role_rows);
        collect_usdm_biomedical_concept_rows(&value, &mut biomedical_concept_rows);
        collect_usdm_reference_integrity_rows(
            &value,
            &mut procedure_rows,
            &mut subject_enrollment_rows,
            &mut document_version_rows,
            &mut substance_rows,
            &mut eligibility_criterion_rows,
            &mut eligibility_criterion_item_rows,
            &mut biospecimen_retention_rows,
            &mut study_element_rows,
            &mut study_arm_rows,
        );
        collect_usdm_cohort_rows(&value, &mut cohort_rows);
        collect_usdm_study_cell_rows(&value, &mut study_cell_rows);
        collect_usdm_condition_rows(&value, &mut condition_rows);
        collect_usdm_parameter_map_rows(&value, &mut parameter_map_rows);
        collect_usdm_syntax_template_text_rows(&value, &mut syntax_template_text_rows);
        collect_usdm_narrative_content_rows(&value, &mut narrative_content_rows);
        collect_usdm_narrative_content_item_rows(&value, &mut narrative_content_item_rows);
        collect_usdm_abbreviation_rows(&value, &mut abbreviation_rows);
        collect_usdm_object_rows(&value, &mut object_rows);
        collect_usdm_geographic_scope_rows(&value, &mut geographic_scope_rows);
        collect_usdm_governance_date_rows(&value, &mut governance_date_rows);
        collect_usdm_document_content_reference_rows(&value, &mut document_content_reference_rows);
        collect_usdm_timeline_rows(&value, &mut timeline_rows);
        collect_usdm_scheduled_instance_rows(&value, &mut scheduled_instance_rows);
        collect_usdm_identifier_rows(&value, &mut identifier_rows);
        collect_usdm_json_schema_issue_rows(&value, &mut json_schema_issue_rows);
    }

    apply_usdm_object_duplicate_flags(&mut object_rows);

    let mut datasets = Vec::new();
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDesignPopulation",
        "usdm-population.json",
        &population_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyRole",
        "usdm-study-role.json",
        &role_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyRoleBlinding",
        "usdm-study-role-blinding.json",
        &role_blinding_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDesign",
        "usdm-study-design.json",
        &design_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "InterventionalStudyDesign",
        "usdm-interventional-study-design.json",
        &interventional_design_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDesignCharacteristicDuplicate",
        "usdm-study-design-characteristic-duplicate.json",
        &design_characteristic_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDesignSubTypeDuplicate",
        "usdm-study-design-sub-type-duplicate.json",
        &design_sub_type_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDesignTherapeuticAreaDuplicate",
        "usdm-study-design-therapeutic-area-duplicate.json",
        &design_therapeutic_area_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyVersion",
        "usdm-study-version.json",
        &version_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Activity",
        "usdm-activity.json",
        &activity_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Duration",
        "usdm-duration.json",
        &duration_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Range",
        "usdm-range.json",
        &range_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "PersonName",
        "usdm-person-name.json",
        &person_name_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Address",
        "usdm-address.json",
        &address_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "AdministrableProduct",
        "usdm-administrable-product.json",
        &administrable_product_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Administration",
        "usdm-administration.json",
        &administration_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Strength",
        "usdm-strength.json",
        &strength_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyAmendmentReason",
        "usdm-study-amendment-reason.json",
        &amendment_reason_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "ProductOrganizationRole",
        "usdm-product-organization-role.json",
        &product_organization_role_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "BiomedicalConcept",
        "usdm-biomedical-concept.json",
        &biomedical_concept_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Procedure",
        "usdm-procedure.json",
        &procedure_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "SubjectEnrollment",
        "usdm-subject-enrollment.json",
        &subject_enrollment_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyDefinitionDocumentVersion",
        "usdm-study-definition-document-version.json",
        &document_version_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Substance",
        "usdm-substance.json",
        &substance_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "EligibilityCriterion",
        "usdm-eligibility-criterion.json",
        &eligibility_criterion_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "EligibilityCriterionItem",
        "usdm-eligibility-criterion-item.json",
        &eligibility_criterion_item_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "BiospecimenRetention",
        "usdm-biospecimen-retention.json",
        &biospecimen_retention_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyElement",
        "usdm-study-element.json",
        &study_element_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyArm",
        "usdm-study-arm.json",
        &study_arm_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyCohort",
        "usdm-study-cohort.json",
        &cohort_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "StudyCell",
        "usdm-study-cell.json",
        &study_cell_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Condition",
        "usdm-condition.json",
        &condition_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "ParameterMap",
        "usdm-parameter-map.json",
        &parameter_map_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "SyntaxTemplateText",
        "usdm-syntax-template-text.json",
        &syntax_template_text_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "NarrativeContent",
        "usdm-narrative-content.json",
        &narrative_content_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "NarrativeContentItem",
        "usdm-narrative-content-item.json",
        &narrative_content_item_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "Abbreviation",
        "usdm-abbreviation.json",
        &abbreviation_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "USDMObject",
        "usdm-object.json",
        &object_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "GeographicScope",
        "usdm-geographic-scope.json",
        &geographic_scope_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "GovernanceDate",
        "usdm-governance-date.json",
        &governance_date_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "DocumentContentReference",
        "usdm-document-content-reference.json",
        &document_content_reference_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "ScheduleTimeline",
        "usdm-schedule-timeline.json",
        &timeline_rows,
    )?;
    push_usdm_dataset(
        data_dir,
        &mut datasets,
        "ScheduledActivityInstance",
        "usdm-scheduled-activity-instance.json",
        &scheduled_instance_rows,
    )?;
    push_usdm_identifier_datasets(data_dir, &mut datasets, &identifier_rows)?;
    push_usdm_dataset_even_when_empty(
        data_dir,
        &mut datasets,
        "JSONSchemaIssue",
        "usdm-json-schema-issue.json",
        &json_schema_issue_rows,
    )?;

    Ok(LoadDataResult {
        datasets,
        warnings: Vec::new(),
    })
}

fn collect_usdm_population_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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
            let Some(population) = design.get("population").filter(|value| value.is_object())
            else {
                continue;
            };
            rows.push(usdm_population_row(
                population,
                design,
                version_index,
                design_index,
            ));
        }
    }
}

fn collect_usdm_role_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(roles) = version.get("roles").and_then(Value::as_array) else {
            continue;
        };
        for (role_index, role) in roles.iter().enumerate() {
            rows.push(usdm_role_row(role, version, version_index, role_index));
        }
    }
}

fn collect_usdm_role_blinding_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let version_id =
            value_string(version.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
        let roles = version
            .get("roles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let designs = version
            .get("studyDesigns")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (role_index, role) in roles.iter().enumerate() {
            let applies_to_ids = string_array(role.get("appliesToIds"));
            for design in &designs {
                let design_id =
                    value_string(design.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
                if applies_to_ids
                    .iter()
                    .any(|id| id == &version_id || id == &design_id)
                {
                    rows.push(usdm_role_blinding_row(
                        role,
                        design,
                        version_index,
                        role_index,
                    ));
                }
            }
        }
    }
}

fn collect_usdm_version_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        rows.push(usdm_version_row(version, version_index));
    }
}

fn collect_usdm_activity_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let bc_category_members = bc_category_member_ids(version);
        let Some(study_designs) = version.get("studyDesigns").and_then(Value::as_array) else {
            continue;
        };
        for (design_index, design) in study_designs.iter().enumerate() {
            let Some(activities) = design.get("activities").and_then(Value::as_array) else {
                continue;
            };
            let activity_ids = activities
                .iter()
                .filter_map(|activity| activity.get("id").and_then(value_string))
                .collect::<HashSet<_>>();
            for (activity_index, activity) in activities.iter().enumerate() {
                let activity_path = format!(
                    "/study/versions/{version_index}/studyDesigns/{design_index}/activities/{activity_index}"
                );
                rows.push(usdm_activity_row(
                    activity,
                    &activity_path,
                    design,
                    activities,
                    &bc_category_members,
                    None,
                    &activity_ids,
                    true,
                ));
                let child_ids = string_array(activity.get("childIds"));
                if child_ids.is_empty() {
                    continue;
                }
                for child_id in child_ids {
                    rows.push(usdm_activity_row(
                        activity,
                        &activity_path,
                        design,
                        activities,
                        &bc_category_members,
                        Some(&child_id),
                        &activity_ids,
                        false,
                    ));
                }
            }
        }
    }
}

fn collect_usdm_biomedical_concept_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (concept_index, concept) in version
            .get("biomedicalConcepts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            rows.push(usdm_biomedical_concept_row(
                concept,
                &format!("/study/versions/{version_index}/biomedicalConcepts/{concept_index}"),
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_usdm_reference_integrity_rows(
    value: &Value,
    procedure_rows: &mut Vec<BTreeMap<String, Value>>,
    subject_enrollment_rows: &mut Vec<BTreeMap<String, Value>>,
    document_version_rows: &mut Vec<BTreeMap<String, Value>>,
    substance_rows: &mut Vec<BTreeMap<String, Value>>,
    eligibility_criterion_rows: &mut Vec<BTreeMap<String, Value>>,
    eligibility_criterion_item_rows: &mut Vec<BTreeMap<String, Value>>,
    biospecimen_retention_rows: &mut Vec<BTreeMap<String, Value>>,
    study_element_rows: &mut Vec<BTreeMap<String, Value>>,
    study_arm_rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(study) = value.get("study") else {
        return;
    };
    let referenced_versions = study
        .get("versions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|version| {
            string_array(version.get("documentVersionIds"))
                .into_iter()
                .chain(
                    version
                        .get("studyDesigns")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .flat_map(|design| string_array(design.get("documentVersionIds"))),
                )
        })
        .collect::<HashSet<_>>();

    for (document_index, document) in study
        .get("documentedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        for (version_index, version) in document
            .get("versions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            document_version_rows.push(usdm_document_version_row(
                version,
                document,
                &format!("/study/documentedBy/{document_index}/versions/{version_index}"),
                &referenced_versions,
            ));
        }
    }

    let Some(versions) = study.get("versions").and_then(Value::as_array) else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let all_interventions = version
            .get("studyInterventions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let site_ids = collect_managed_site_ids(version);
        let cohort_ids = collect_nested_ids(version, "cohorts");
        let referenced_criterion_items = version
            .get("studyDesigns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|design| {
                design
                    .get("eligibilityCriteria")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|criterion| criterion.get("criterionItemId").and_then(value_string))
            })
            .collect::<HashSet<_>>();

        for (item_index, item) in version
            .get("eligibilityCriterionItems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            eligibility_criterion_item_rows.push(usdm_eligibility_item_row(
                item,
                version,
                &format!("/study/versions/{version_index}/eligibilityCriterionItems/{item_index}"),
                &referenced_criterion_items,
            ));
        }

        for (amendment_index, amendment) in version
            .get("amendments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (enrollment_index, enrollment) in amendment
                .get("enrollments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                subject_enrollment_rows.push(usdm_subject_enrollment_row(
                    enrollment,
                    amendment,
                    &site_ids,
                    &cohort_ids,
                    &format!("/study/versions/{version_index}/amendments/{amendment_index}/enrollments/{enrollment_index}"),
                ));
            }
        }

        for (product_index, product) in version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (ingredient_index, ingredient) in product
                .get("ingredients")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(substance) = ingredient.get("substance") {
                    collect_usdm_substance_reference_rows(
                        substance,
                        product,
                        ingredient,
                        substance,
                        &format!("/study/versions/{version_index}/administrableProducts/{product_index}/ingredients/{ingredient_index}/substance"),
                        substance_rows,
                    );
                }
            }
        }

        for (design_index, design) in version
            .get("studyDesigns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let design_intervention_ids = string_array(design.get("studyInterventionIds"));
            let indication_ids = collect_direct_ids(design.get("indications"));
            let population_id = design
                .get("population")
                .and_then(|population| population.get("id"))
                .and_then(value_string);
            let population_cohort_ids = design
                .get("population")
                .and_then(|population| population.get("cohorts"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|cohort| cohort.get("id").and_then(value_string))
                .collect::<HashSet<_>>();
            let population_criterion_ids = string_array(
                design
                    .get("population")
                    .and_then(|population| population.get("criterionIds")),
            )
            .into_iter()
            .collect::<HashSet<_>>();
            let cohort_criterion_ids = design
                .get("population")
                .and_then(|population| population.get("cohorts"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|cohort| string_array(cohort.get("criterionIds")))
                .collect::<HashSet<_>>();
            let mut criterion_item_counts: HashMap<String, usize> = HashMap::new();
            for criterion in design
                .get("eligibilityCriteria")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(item_id) = criterion.get("criterionItemId").and_then(value_string) {
                    *criterion_item_counts.entry(item_id).or_default() += 1;
                }
            }

            for (activity_index, activity) in design
                .get("activities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                for (procedure_index, procedure) in activity
                    .get("definedProcedures")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    procedure_rows.push(usdm_procedure_row(
                        procedure,
                        activity,
                        design,
                        &all_interventions,
                        &design_intervention_ids,
                        &format!("/study/versions/{version_index}/studyDesigns/{design_index}/activities/{activity_index}/definedProcedures/{procedure_index}"),
                    ));
                }
            }

            for (retention_index, retention) in design
                .get("biospecimenRetentions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                biospecimen_retention_rows.push(usdm_biospecimen_retention_row(
                    retention,
                    design,
                    &format!("/study/versions/{version_index}/studyDesigns/{design_index}/biospecimenRetentions/{retention_index}"),
                ));
            }

            for (element_index, element) in design
                .get("elements")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let intervention_parent_designs = study_intervention_parent_designs(version);
                study_element_rows.push(usdm_study_element_row(
                    element,
                    design,
                    &all_interventions,
                    &design_intervention_ids,
                    &intervention_parent_designs,
                    &format!("/study/versions/{version_index}/studyDesigns/{design_index}/elements/{element_index}"),
                ));
            }

            for (arm_index, arm) in design
                .get("arms")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                study_arm_rows.push(usdm_study_arm_row(
                    arm,
                    design,
                    population_id.as_deref(),
                    &population_cohort_ids,
                    &format!("/study/versions/{version_index}/studyDesigns/{design_index}/arms/{arm_index}"),
                ));
            }

            if let Some(population) = design.get("population") {
                for (cohort_index, cohort) in population
                    .get("cohorts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    eligibility_criterion_rows.push(usdm_cohort_indication_row(
                        cohort,
                        population,
                        design,
                        &indication_ids,
                        &format!("/study/versions/{version_index}/studyDesigns/{design_index}/population/cohorts/{cohort_index}"),
                    ));
                }
            }

            for (criterion_index, criterion) in design
                .get("eligibilityCriteria")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                eligibility_criterion_rows.push(usdm_eligibility_criterion_row(
                    criterion,
                    design,
                    &population_criterion_ids,
                    &cohort_criterion_ids,
                    &criterion_item_counts,
                    &format!("/study/versions/{version_index}/studyDesigns/{design_index}/eligibilityCriteria/{criterion_index}"),
                ));
            }
        }
    }
}

fn collect_usdm_substance_reference_rows(
    value: &Value,
    product: &Value,
    ingredient: &Value,
    parent_substance: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("referenceSubstance") {
                rows.push(usdm_substance_row(
                    reference,
                    product,
                    ingredient,
                    parent_substance,
                    &format!("{path}/referenceSubstance"),
                ));
            }
            for (key, child) in object {
                if key != "referenceSubstance" {
                    collect_usdm_substance_reference_rows(
                        child,
                        product,
                        ingredient,
                        parent_substance,
                        &format!("{path}/{key}"),
                        rows,
                    );
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_substance_reference_rows(
                    child,
                    product,
                    ingredient,
                    parent_substance,
                    &format!("{path}/{index}"),
                    rows,
                );
            }
        }
        _ => {}
    }
}

fn usdm_population_row(
    population: &Value,
    design: &Value,
    version_index: usize,
    design_index: usize,
) -> BTreeMap<String, Value> {
    let enrollment = population
        .get("plannedEnrollmentNumber")
        .unwrap_or(&Value::Null);
    let completion = population
        .get("plannedCompletionNumber")
        .unwrap_or(&Value::Null);
    let cohorts = population
        .get("cohorts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/studyDesigns/{design_index}/population"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(population.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(population.get("id")));
    row.insert("name".to_owned(), json_string(population.get("name")));
    row.insert("StudyDesign.id".to_owned(), json_string(design.get("id")));
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(design.get("name")),
    );
    insert_quantity_columns(&mut row, "plannedEnrollmentNumber", enrollment, &cohorts);
    insert_quantity_columns(&mut row, "plannedCompletionNumber", completion, &cohorts);
    insert_planned_sex_columns(&mut row, population.get("plannedSex"));
    row
}

fn usdm_role_row(
    role: &Value,
    version: &Value,
    version_index: usize,
    role_index: usize,
) -> BTreeMap<String, Value> {
    let applies_to_ids = string_array(role.get("appliesToIds"));
    let organization_ids = string_array(role.get("organizationIds"));
    let assigned_persons = role
        .get("assignedPersons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let version_id = value_string(version.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
    let study_design_ids = string_array(
        version
            .get("studyDesigns")
            .map(|designs| {
                Value::Array(
                    designs
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|design| design.get("id").cloned())
                        .collect(),
                )
            })
            .as_ref(),
    );
    let organizations = version
        .get("organizations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let applies_to_version_only = applies_to_ids.len() == 1 && applies_to_ids[0] == version_id;
    let applies_to_designs_only = !applies_to_ids.is_empty()
        && applies_to_ids
            .iter()
            .all(|id| study_design_ids.iter().any(|design_id| design_id == id));
    let valid_organization_count = organization_ids
        .iter()
        .filter(|id| {
            organizations
                .iter()
                .any(|org| value_string(org.get("id").unwrap_or(&Value::Null)).as_ref() == Some(id))
        })
        .count();

    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/roles/{role_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(role.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(role.get("id")));
    row.insert("name".to_owned(), json_string(role.get("name")));
    row.insert(
        "code.code".to_owned(),
        json_string(role.get("code").and_then(|code| code.get("code"))),
    );
    row.insert(
        "code.decode".to_owned(),
        json_string(role.get("code").and_then(|code| code.get("decode"))),
    );
    row.insert(
        "code".to_owned(),
        Value::String(format_code(role.get("code"))),
    );
    row.insert(
        "appliesToIds".to_owned(),
        Value::String(format_string_list(&applies_to_ids)),
    );
    row.insert(
        "StudyVersion.id".to_owned(),
        Value::String(version_id.clone()),
    );
    row.insert(
        "StudyVersion.studyDesigns.id".to_owned(),
        Value::String(format_string_list(&study_design_ids)),
    );
    row.insert(
        "sponsor_role_applies_to_study_version".to_owned(),
        Value::Bool(applies_to_ids.iter().any(|id| id == &version_id)),
    );
    row.insert(
        "organizationIds".to_owned(),
        Value::String(format_organization_ids(&organization_ids, &organizations)),
    );
    row.insert(
        "assignedPersons".to_owned(),
        Value::String(format_assigned_persons(&assigned_persons)),
    );
    row.insert(
        "organizationIds.count".to_owned(),
        Value::Number(serde_json::Number::from(organization_ids.len())),
    );
    row.insert(
        "# Valid Organizations".to_owned(),
        Value::Number(serde_json::Number::from(valid_organization_count)),
    );
    row.insert(
        "sponsor_role_has_exactly_one_valid_org".to_owned(),
        Value::Bool(organization_ids.len() == 1 && valid_organization_count == 1),
    );
    row.insert(
        "study_role_has_assigned_persons_and_orgs".to_owned(),
        Value::Bool(!assigned_persons.is_empty() && !organization_ids.is_empty()),
    );
    row.insert(
        "study_role_invalid_applies_to_scope".to_owned(),
        Value::Bool(!(applies_to_version_only || applies_to_designs_only)),
    );
    row
}

fn usdm_role_blinding_row(
    role: &Value,
    design: &Value,
    version_index: usize,
    role_index: usize,
) -> BTreeMap<String, Value> {
    let applies_to_ids = string_array(role.get("appliesToIds"));
    let is_masked = role
        .get("masking")
        .and_then(|masking| masking.get("isMasked"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let design_blinding_code = design
        .get("blindingSchema")
        .and_then(|schema| schema.get("standardCode"))
        .and_then(|code| code.get("code"));
    let design_blinding_decode = design
        .get("blindingSchema")
        .and_then(|schema| schema.get("standardCode"))
        .and_then(|code| code.get("decode"));
    let open_label = design_blinding_code
        .and_then(Value::as_str)
        .is_some_and(|code| code == "C49659");

    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/roles/{role_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(role.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(role.get("id")));
    row.insert("name".to_owned(), json_string(role.get("name")));
    row.insert(
        "code".to_owned(),
        json_string(role.get("code").and_then(|code| code.get("decode"))),
    );
    row.insert(
        "masking.text".to_owned(),
        json_string(role.get("masking").and_then(|masking| masking.get("text"))),
    );
    row.insert("masking.isMasked".to_owned(), Value::Bool(is_masked));
    row.insert(
        "appliesToIds".to_owned(),
        Value::String(format_string_list(&applies_to_ids)),
    );
    row.insert("StudyDesign.id".to_owned(), json_string(design.get("id")));
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(design.get("name")),
    );
    row.insert(
        "StudyDesign.blindingSchema".to_owned(),
        json_string(design_blinding_decode),
    );
    row.insert(
        "study_role_masked_for_open_label_design".to_owned(),
        Value::Bool(open_label && is_masked),
    );
    row
}

fn usdm_version_row(version: &Value, version_index: usize) -> BTreeMap<String, Value> {
    let duplicate_document_version_ids =
        duplicate_strings(&string_array(version.get("documentVersionIds")));
    let sponsor_roles = version
        .get("roles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|role| {
            role.get("code")
                .and_then(|code| code.get("code"))
                .and_then(value_string)
                .as_deref()
                == Some("C70793")
        })
        .collect::<Vec<_>>();
    let sponsor_org_ids = version
        .get("roles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|role| {
            role.get("code")
                .and_then(|code| code.get("code"))
                .and_then(value_string)
                .as_deref()
                == Some("C70793")
        })
        .flat_map(|role| string_array(role.get("organizationIds")))
        .collect::<Vec<_>>();
    let sponsor_identifiers = version
        .get("studyIdentifiers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|identifier| {
            identifier
                .get("scopeId")
                .and_then(value_string)
                .is_some_and(|scope| sponsor_org_ids.iter().any(|id| id == &scope))
        })
        .collect::<Vec<_>>();

    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!("/study/versions/{version_index}")),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(version.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(version.get("id")));
    row.insert(
        "versionIdentifier".to_owned(),
        json_string(version.get("versionIdentifier")),
    );
    row.insert(
        "Duplicate documentVersionIds".to_owned(),
        Value::String(format_string_list(&duplicate_document_version_ids)),
    );
    row.insert(
        "duplicate_document_version_ids".to_owned(),
        Value::Bool(!duplicate_document_version_ids.is_empty()),
    );
    row.insert(
        "# Sponsor Identifiers".to_owned(),
        Value::Number(serde_json::Number::from(sponsor_identifiers.len())),
    );
    row.insert(
        "Sponsor Identifiers".to_owned(),
        Value::String(format_sponsor_identifiers(&sponsor_identifiers, version)),
    );
    row.insert(
        "# Sponsor Roles".to_owned(),
        Value::Number(serde_json::Number::from(sponsor_roles.len())),
    );
    row.insert(
        "Sponsor Roles".to_owned(),
        Value::String(format_sponsor_roles(&sponsor_roles)),
    );
    row
}

#[allow(clippy::too_many_arguments)]
fn usdm_activity_row(
    activity: &Value,
    path: &str,
    study_design: &Value,
    activities: &[Value],
    bc_category_members: &BTreeMap<String, BTreeSet<String>>,
    child_id: Option<&str>,
    activity_ids: &HashSet<String>,
    summary_row: bool,
) -> BTreeMap<String, Value> {
    let invalid_child_id = child_id.is_some_and(|id| !activity_ids.contains(id));
    let child_ids = string_array(activity.get("childIds"));
    let biomedical_concept_ids = string_array(activity.get("biomedicalConceptIds"));
    let bc_category_ids = string_array(activity.get("bcCategoryIds"));
    let bc_surrogate_ids = string_array(activity.get("bcSurrogateIds"));
    let defined_procedure_ids = activity
        .get("definedProcedures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|procedure| procedure.get("id").and_then(value_string))
        .collect::<Vec<_>>();
    let timeline_id = activity.get("timelineId").and_then(value_string);
    let activity_id = activity
        .get("id")
        .and_then(value_string)
        .unwrap_or_default();
    let previous_id = activity.get("previousId").and_then(value_string);
    let next_id = activity.get("nextId").and_then(value_string);
    let has_details = !biomedical_concept_ids.is_empty()
        || !bc_category_ids.is_empty()
        || !bc_surrogate_ids.is_empty()
        || !defined_procedure_ids.is_empty()
        || timeline_id.as_ref().is_some_and(|value| !value.is_empty());
    let overlapping_biomedical_concepts = biomedical_concept_ids
        .iter()
        .filter(|biomedical_concept_id| {
            bc_category_ids.iter().any(|category_id| {
                bc_category_members
                    .get(category_id)
                    .is_some_and(|members| members.contains(*biomedical_concept_id))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let overlapping_categories = bc_category_ids
        .iter()
        .filter(|category_id| {
            bc_category_members
                .get(*category_id)
                .is_some_and(|members| {
                    overlapping_biomedical_concepts
                        .iter()
                        .any(|biomedical_concept_id| members.contains(biomedical_concept_id))
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let has_children = !child_ids.is_empty();
    let parent_activities = activities
        .iter()
        .filter(|candidate| string_array(candidate.get("childIds")).contains(&activity_id))
        .collect::<Vec<_>>();
    let parent_activity_ids = parent_activities
        .iter()
        .filter_map(|parent| parent.get("id").and_then(value_string))
        .collect::<Vec<_>>();
    let other_parent_descendant_ids = parent_activities
        .iter()
        .flat_map(|parent| {
            string_array(parent.get("childIds"))
                .into_iter()
                .filter(|id| id != &activity_id)
                .flat_map(|id| {
                    let mut ids = vec![id.clone()];
                    collect_activity_descendant_ids(activities, &id, &mut ids);
                    ids
                })
        })
        .collect::<Vec<_>>();
    let previous_child_ids = previous_id
        .as_deref()
        .and_then(|previous_id| {
            activities.iter().find(|candidate| {
                candidate.get("id").and_then(value_string).as_deref() == Some(previous_id)
            })
        })
        .map(|previous| string_array(previous.get("childIds")))
        .unwrap_or_default();
    let activity_child_order_invalid = (has_children
        && next_id
            .as_ref()
            .is_none_or(|next_id| !child_ids.contains(next_id)))
        || (!parent_activity_ids.is_empty()
            && previous_id.as_ref().is_none_or(|previous_id| {
                !parent_activity_ids.contains(previous_id)
                    && !other_parent_descendant_ids.contains(previous_id)
            }))
        || (!previous_child_ids.is_empty() && !previous_child_ids.contains(&activity_id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(activity.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(activity.get("id")));
    row.insert("name".to_owned(), json_string(activity.get("name")));
    row.insert(
        "childId".to_owned(),
        child_id
            .map(|id| Value::String(id.to_owned()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "childIds".to_owned(),
        Value::String(format_string_list(&child_ids)),
    );
    row.insert(
        "previousId".to_owned(),
        previous_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "nextId".to_owned(),
        next_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "Previous Activity's childIds".to_owned(),
        Value::String(format_string_list(&previous_child_ids)),
    );
    row.insert(
        "Parent Activity's id".to_owned(),
        Value::String(format_string_list(&parent_activity_ids)),
    );
    row.insert(
        "Parent Activity's other descendants' ids".to_owned(),
        Value::String(format_string_list(&other_parent_descendant_ids)),
    );
    row.insert(
        "biomedicalConceptIds".to_owned(),
        Value::String(format_string_list(&biomedical_concept_ids)),
    );
    row.insert(
        "bcCategoryIds".to_owned(),
        Value::String(format_string_list(&bc_category_ids)),
    );
    row.insert(
        "biomedicalConceptId".to_owned(),
        Value::String(format_string_list(&overlapping_biomedical_concepts)),
    );
    row.insert(
        "bcCategoryId(s) containing BC".to_owned(),
        Value::String(format_string_list(&overlapping_categories)),
    );
    row.insert(
        "bcSurrogateIds".to_owned(),
        Value::String(format_string_list(&bc_surrogate_ids)),
    );
    row.insert(
        "timelineId".to_owned(),
        timeline_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "definedProcedures.id".to_owned(),
        if defined_procedure_ids.is_empty() {
            Value::Null
        } else {
            Value::String(defined_procedure_ids.join("; "))
        },
    );
    row.insert(
        "StudyDesign.id".to_owned(),
        json_string(study_design.get("id")),
    );
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(study_design.get("name")),
    );
    row.insert(
        "activity_child_id_invalid".to_owned(),
        Value::Bool(invalid_child_id),
    );
    row.insert("activity_summary_row".to_owned(), Value::Bool(summary_row));
    row.insert(
        "activity_children_with_details".to_owned(),
        Value::Bool(summary_row && has_children && has_details),
    );
    row.insert(
        "activity_bc_category_overlap".to_owned(),
        Value::Bool(!overlapping_biomedical_concepts.is_empty()),
    );
    row.insert(
        "activity_child_order_invalid".to_owned(),
        Value::Bool(activity_child_order_invalid),
    );
    row
}

fn bc_category_member_ids(version: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let categories = version
        .get("bcCategories")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    categories
        .iter()
        .filter_map(|category| {
            let id = category.get("id").and_then(value_string)?;
            let mut members = BTreeSet::new();
            collect_bc_category_members(categories, category, &mut members);
            Some((id, members))
        })
        .collect()
}

fn collect_bc_category_members(
    categories: &[Value],
    category: &Value,
    members: &mut BTreeSet<String>,
) {
    for member_id in string_array(category.get("memberIds")) {
        members.insert(member_id);
    }
    for child_id in string_array(category.get("childIds")) {
        if let Some(child) = categories.iter().find(|candidate| {
            candidate.get("id").and_then(value_string).as_deref() == Some(&child_id)
        }) {
            collect_bc_category_members(categories, child, members);
        }
    }
}

fn collect_activity_descendant_ids(activities: &[Value], id: &str, descendants: &mut Vec<String>) {
    let Some(activity) = activities
        .iter()
        .find(|candidate| candidate.get("id").and_then(value_string).as_deref() == Some(id))
    else {
        return;
    };
    for child_id in string_array(activity.get("childIds")) {
        if descendants.contains(&child_id) {
            continue;
        }
        descendants.push(child_id.clone());
        collect_activity_descendant_ids(activities, &child_id, descendants);
    }
}

fn usdm_biomedical_concept_row(concept: &Value, path: &str) -> BTreeMap<String, Value> {
    let label = concept
        .get("label")
        .and_then(value_string)
        .unwrap_or_default();
    let synonyms = string_array(concept.get("synonyms"));
    let duplicate = !label.is_empty()
        && synonyms
            .iter()
            .any(|synonym| synonym.eq_ignore_ascii_case(&label));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(concept.get("name")));
    row.insert("label/synonym".to_owned(), Value::String(label));
    row.insert(
        "synonyms".to_owned(),
        Value::String(format_string_list(&synonyms)),
    );
    row.insert(
        "biomedical_concept_synonym_equals_label".to_owned(),
        Value::Bool(duplicate),
    );
    row
}

fn usdm_document_version_row(
    version: &Value,
    document: &Value,
    path: &str,
    referenced_versions: &HashSet<String>,
) -> BTreeMap<String, Value> {
    let id = version.get("id").and_then(value_string).unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "StudyDefinitionDocument.id".to_owned(),
        json_string(document.get("id")),
    );
    row.insert(
        "StudyDefinitionDocument.name".to_owned(),
        json_string(document.get("name")),
    );
    row.insert("version".to_owned(), json_string(version.get("version")));
    row.insert(
        "study_definition_document_version_unreferenced".to_owned(),
        Value::Bool(!id.is_empty() && !referenced_versions.contains(&id)),
    );
    row
}

fn usdm_procedure_row(
    procedure: &Value,
    activity: &Value,
    design: &Value,
    all_interventions: &[Value],
    design_intervention_ids: &[String],
    path: &str,
) -> BTreeMap<String, Value> {
    let intervention_id = procedure.get("studyInterventionId").and_then(value_string);
    let invalid = intervention_id
        .as_deref()
        .is_some_and(|id| !design_intervention_ids.iter().any(|valid| valid == id));
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("Activity.id".to_owned(), json_string(activity.get("id")));
    row.insert(
        "Activity.name".to_owned(),
        json_string(activity.get("name")),
    );
    row.insert("name".to_owned(), json_string(procedure.get("name")));
    row.insert(
        "StudyDesign.studyInterventionIds".to_owned(),
        Value::String(format_string_list(design_intervention_ids)),
    );
    row.insert(
        "studyInterventionId".to_owned(),
        intervention_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "StudyIntervention.name".to_owned(),
        intervention_id
            .as_deref()
            .and_then(|id| named_usdm_object_name(all_interventions, id))
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "procedure_invalid_study_intervention".to_owned(),
        Value::Bool(invalid),
    );
    row
}

fn usdm_subject_enrollment_row(
    enrollment: &Value,
    amendment: &Value,
    site_ids: &HashSet<String>,
    cohort_ids: &HashSet<String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let has_geo = value_exists(enrollment.get("forGeographicScope"));
    let site_id = enrollment.get("forStudySiteId").and_then(value_string);
    let cohort_id = enrollment.get("forStudyCohortId").and_then(value_string);
    let has_site = site_id.as_deref().is_some_and(|id| !id.is_empty());
    let has_cohort = cohort_id.as_deref().is_some_and(|id| !id.is_empty());
    let selected = [has_geo, has_site, has_cohort]
        .into_iter()
        .filter(|selected| *selected)
        .count();
    let invalid_site = has_site && !site_id.as_ref().is_some_and(|id| site_ids.contains(id));
    let invalid_cohort =
        has_cohort && !cohort_id.as_ref().is_some_and(|id| cohort_ids.contains(id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "StudyAmendment.id".to_owned(),
        json_string(amendment.get("id")),
    );
    row.insert(
        "StudyAmendment.name".to_owned(),
        json_string(amendment.get("name")),
    );
    row.insert("name".to_owned(), json_string(enrollment.get("name")));
    row.insert(
        "forGeographicScope".to_owned(),
        json_string(enrollment.get("forGeographicScope")),
    );
    row.insert(
        "forStudySiteId".to_owned(),
        site_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "forStudyCohortId".to_owned(),
        cohort_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "subject_enrollment_invalid_scope".to_owned(),
        Value::Bool(selected != 1 || invalid_site || invalid_cohort),
    );
    row
}

fn usdm_substance_row(
    substance: &Value,
    product: &Value,
    ingredient: &Value,
    parent_substance: &Value,
    path: &str,
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "AdministrableProduct.id".to_owned(),
        json_string(product.get("id")),
    );
    row.insert(
        "AdministrableProduct.name".to_owned(),
        json_string(product.get("name")),
    );
    row.insert(
        "Ingredient.id".to_owned(),
        json_string(ingredient.get("id")),
    );
    row.insert(
        "Parent Substance.id".to_owned(),
        json_string(parent_substance.get("id")),
    );
    row.insert(
        "Parent Substance.name".to_owned(),
        json_string(parent_substance.get("name")),
    );
    row.insert("name".to_owned(), json_string(substance.get("name")));
    row.insert(
        "referenceSubstance.id".to_owned(),
        json_string(
            substance
                .get("referenceSubstance")
                .and_then(|value| value.get("id")),
        ),
    );
    row.insert(
        "referenceSubstance.name".to_owned(),
        json_string(
            substance
                .get("referenceSubstance")
                .and_then(|value| value.get("name")),
        ),
    );
    row.insert(
        "substance_reference_has_reference".to_owned(),
        Value::Bool(value_exists(substance.get("referenceSubstance"))),
    );
    row
}

fn usdm_eligibility_criterion_row(
    criterion: &Value,
    design: &Value,
    population_criterion_ids: &HashSet<String>,
    cohort_criterion_ids: &HashSet<String>,
    criterion_item_counts: &HashMap<String, usize>,
    path: &str,
) -> BTreeMap<String, Value> {
    let id = criterion
        .get("id")
        .and_then(value_string)
        .unwrap_or_default();
    let criterion_item_id = criterion.get("criterionItemId").and_then(value_string);
    let used_in_population = population_criterion_ids.contains(&id);
    let used_in_cohort = cohort_criterion_ids.contains(&id);
    let duplicate_item = criterion_item_id
        .as_ref()
        .and_then(|id| criterion_item_counts.get(id))
        .is_some_and(|count| *count > 1);
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(criterion.get("name")));
    row.insert(
        "category".to_owned(),
        json_string(
            criterion
                .get("category")
                .and_then(|value| value.get("decode")),
        ),
    );
    row.insert(
        "identifier".to_owned(),
        json_string(criterion.get("identifier")),
    );
    row.insert(
        "criterionItemId".to_owned(),
        criterion_item_id.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "eligibility_criterion_unused".to_owned(),
        Value::Bool(!id.is_empty() && !used_in_population && !used_in_cohort),
    );
    row.insert(
        "eligibility_criterion_used_in_population_and_cohort".to_owned(),
        Value::Bool(used_in_population && used_in_cohort),
    );
    row.insert(
        "eligibility_criterion_duplicate_item".to_owned(),
        Value::Bool(duplicate_item),
    );
    row
}

fn usdm_cohort_indication_row(
    cohort: &Value,
    population: &Value,
    design: &Value,
    indication_ids: &HashSet<String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let invalid = string_array(cohort.get("indicationIds"))
        .into_iter()
        .filter(|id| !indication_ids.contains(id))
        .collect::<Vec<_>>();
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(cohort.get("name")));
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
        Value::String(format_string_list(&invalid)),
    );
    row.insert(
        "study_cohort_invalid_indication".to_owned(),
        Value::Bool(!invalid.is_empty()),
    );
    row
}

fn usdm_eligibility_item_row(
    item: &Value,
    version: &Value,
    path: &str,
    referenced_items: &HashSet<String>,
) -> BTreeMap<String, Value> {
    let id = item.get("id").and_then(value_string).unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("StudyVersion.id".to_owned(), json_string(version.get("id")));
    row.insert(
        "StudyVersion.versionIdentifier".to_owned(),
        json_string(version.get("versionIdentifier")),
    );
    row.insert("name".to_owned(), json_string(item.get("name")));
    row.insert(
        "eligibility_criterion_item_unused".to_owned(),
        Value::Bool(!id.is_empty() && !referenced_items.contains(&id)),
    );
    row
}

fn usdm_biospecimen_retention_row(
    retention: &Value,
    design: &Value,
    path: &str,
) -> BTreeMap<String, Value> {
    let is_retained = retention
        .get("isRetained")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let includes_dna_known = retention
        .get("includesDNA")
        .and_then(Value::as_bool)
        .is_some();
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(retention.get("name")));
    row.insert("isRetained".to_owned(), Value::Bool(is_retained));
    row.insert(
        "biospecimen_retained_missing_includes_dna".to_owned(),
        Value::Bool(is_retained && !includes_dna_known),
    );
    row
}

fn usdm_study_element_row(
    element: &Value,
    design: &Value,
    all_interventions: &[Value],
    design_intervention_ids: &[String],
    intervention_parent_designs: &HashMap<String, String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let design_id = design.get("id").and_then(value_string).unwrap_or_default();
    let invalid = string_array(element.get("studyInterventionIds"))
        .into_iter()
        .filter(|id| !design_intervention_ids.iter().any(|valid| valid == id))
        .collect::<Vec<_>>();
    let embedded_intervention_ids = collect_direct_ids(design.get("studyInterventions"));
    let cross_design_interventions = string_array(element.get("studyInterventionIds"))
        .into_iter()
        .filter(|id| !embedded_intervention_ids.contains(id))
        .collect::<Vec<_>>();
    let parent_designs = cross_design_interventions
        .iter()
        .map(|id| {
            intervention_parent_designs
                .get(id)
                .cloned()
                .unwrap_or_else(|| "[Invalid studyInterventionIds value]".to_owned())
        })
        .collect::<Vec<_>>();
    let invalid_names = invalid
        .iter()
        .map(|id| {
            let name = named_usdm_object_name(all_interventions, id)
                .unwrap_or_else(|| "Not defined".to_owned());
            format!("{id}: {name}")
        })
        .collect::<Vec<_>>();
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(element.get("name")));
    row.insert(
        "StudyDesign.studyInterventionIds".to_owned(),
        Value::String(format_string_list(design_intervention_ids)),
    );
    row.insert(
        "Invalid studyInterventionIds".to_owned(),
        Value::String(format_string_list(&invalid)),
    );
    row.insert(
        "Invalid StudyIntervention.name".to_owned(),
        Value::String(format_string_list(&invalid_names)),
    );
    row.insert(
        "studyInterventionIds value".to_owned(),
        Value::String(format_string_list(&cross_design_interventions)),
    );
    row.insert(
        "Referenced intervention's parent StudyDesign.id".to_owned(),
        Value::String(format_string_list(&parent_designs)),
    );
    row.insert(
        "study_element_invalid_study_intervention".to_owned(),
        Value::Bool(!invalid.is_empty()),
    );
    row.insert(
        "study_element_cross_design_study_intervention".to_owned(),
        Value::Bool(
            !cross_design_interventions.is_empty()
                && parent_designs.iter().any(|parent| parent != &design_id),
        ),
    );
    row
}

fn study_intervention_parent_designs(version: &Value) -> HashMap<String, String> {
    let mut parents = HashMap::new();
    for design in version
        .get("studyDesigns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let design_id = design.get("id").and_then(value_string).unwrap_or_default();
        for intervention in design
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = intervention.get("id").and_then(value_string) {
                parents.insert(id, design_id.clone());
            }
        }
    }
    parents
}

fn usdm_study_arm_row(
    arm: &Value,
    design: &Value,
    population_id: Option<&str>,
    cohort_ids: &HashSet<String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let arm_id = arm.get("id").and_then(value_string).unwrap_or_default();
    let invalid = string_array(arm.get("populationIds"))
        .into_iter()
        .filter(|id| Some(id.as_str()) != population_id && !cohort_ids.contains(id))
        .collect::<Vec<_>>();
    let epochs = design
        .get("epochs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let study_cells = design
        .get("studyCells")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let arm_epoch_ids = study_cells
        .iter()
        .filter(|cell| cell.get("armId").and_then(value_string).as_deref() == Some(&arm_id))
        .filter_map(|cell| cell.get("epochId").and_then(value_string))
        .collect::<HashSet<_>>();
    let study_design_epochs = epochs
        .iter()
        .filter_map(format_usdm_id_name)
        .collect::<Vec<_>>();
    let arm_study_cell_epoch_refs = study_cells
        .iter()
        .filter(|cell| cell.get("armId").and_then(value_string).as_deref() == Some(&arm_id))
        .filter_map(|cell| {
            let cell_id = cell.get("id").and_then(value_string)?;
            let epoch_id = cell
                .get("epochId")
                .and_then(value_string)
                .unwrap_or_default();
            let epoch_name = named_usdm_object_name(epochs, &epoch_id)
                .unwrap_or_else(|| "Invalid epochId".to_owned());
            Some(format!("{cell_id}: {epoch_id} ({epoch_name})"))
        })
        .collect::<Vec<_>>();
    let missing_epoch_refs = epochs
        .iter()
        .filter(|epoch| {
            epoch
                .get("id")
                .and_then(value_string)
                .is_some_and(|id| !arm_epoch_ids.contains(&id))
        })
        .filter_map(format_usdm_id_name)
        .collect::<Vec<_>>();
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("id".to_owned(), json_string(arm.get("id")));
    row.insert(
        "instanceType".to_owned(),
        json_string(arm.get("instanceType")),
    );
    row.insert("name".to_owned(), json_string(arm.get("name")));
    row.insert(
        "StudyDesign.epochs".to_owned(),
        Value::String(format_semicolon_list(&study_design_epochs)),
    );
    row.insert(
        "Arm's StudyCell Epoch Refs".to_owned(),
        Value::String(format_semicolon_list(&arm_study_cell_epoch_refs)),
    );
    row.insert(
        "Missing Epoch Refs".to_owned(),
        Value::String(format_semicolon_list(&missing_epoch_refs)),
    );
    row.insert(
        "StudyDesign.population.id".to_owned(),
        population_id
            .map(|id| Value::String(id.to_owned()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "StudyDesign.population.cohorts.id".to_owned(),
        Value::String(format_string_list(
            &cohort_ids.iter().cloned().collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "populationId".to_owned(),
        invalid
            .first()
            .map(|id| Value::String(id.clone()))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "populationId.invalid_count".to_owned(),
        Value::Number(serde_json::Number::from(invalid.len())),
    );
    row.insert(
        "study_arm_invalid_population".to_owned(),
        Value::Bool(!invalid.is_empty()),
    );
    row.insert(
        "study_arm_missing_epoch_refs".to_owned(),
        Value::Bool(!missing_epoch_refs.is_empty()),
    );
    row
}

fn insert_study_design_context(row: &mut BTreeMap<String, Value>, design: &Value) {
    row.insert("StudyDesign.id".to_owned(), json_string(design.get("id")));
    row.insert(
        "StudyDesign.name".to_owned(),
        json_string(design.get("name")),
    );
}

fn format_organization_ids(ids: &[String], organizations: &[Value]) -> String {
    if ids.is_empty() {
        return String::new();
    }

    format!(
        "[{}]",
        ids.iter()
            .map(|id| {
                let name = organizations
                    .iter()
                    .find(|organization| {
                        value_string(organization.get("id").unwrap_or(&Value::Null)).as_ref()
                            == Some(id)
                    })
                    .and_then(|organization| organization.get("name"))
                    .and_then(value_string)
                    .unwrap_or_else(|| "Invalid organizationId".to_owned());
                format!("{id}: {name}")
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn format_assigned_persons(persons: &[&Value]) -> String {
    if persons.is_empty() {
        return String::new();
    }

    format!(
        "[{}]",
        persons
            .iter()
            .map(|person| {
                let id = person.get("id").and_then(value_string).unwrap_or_default();
                let name = person
                    .get("name")
                    .and_then(value_string)
                    .unwrap_or_default();
                format!("{id}: {name}")
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn format_sponsor_identifiers(identifiers: &[&Value], version: &Value) -> String {
    if identifiers.is_empty() {
        return "null".to_owned();
    }

    identifiers
        .iter()
        .map(|identifier| {
            let id = value_string(identifier.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
            let text =
                value_string(identifier.get("text").unwrap_or(&Value::Null)).unwrap_or_default();
            let scope =
                value_string(identifier.get("scopeId").unwrap_or(&Value::Null)).unwrap_or_default();
            let org_name = version
                .get("organizations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|organization| {
                    value_string(organization.get("id").unwrap_or(&Value::Null)).as_deref()
                        == Some(scope.as_str())
                })
                .and_then(|organization| organization.get("name"))
                .and_then(value_string)
                .unwrap_or_default();
            format!("{id}: {text} ({scope}: {org_name})")
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn format_sponsor_roles(roles: &[&Value]) -> String {
    if roles.is_empty() {
        return "null".to_owned();
    }

    format!(
        "[{}]",
        roles
            .iter()
            .map(|role| {
                let id = value_string(role.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
                let code = format_code(role.get("code"));
                format!("{id}: {code}")
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}
