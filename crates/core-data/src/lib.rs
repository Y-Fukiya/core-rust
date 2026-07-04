#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use polars::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

mod csv_dataset;
mod csv_records;
mod dataset_joins;
mod dataset_operations;
mod dataset_package;
mod dataset_paths;
mod dataset_transforms;
mod json_table;
mod open_rules_data_dir;
mod open_rules_variables;
mod row_key;
mod usdm_abbreviations;
mod usdm_collectors;
mod usdm_content;
mod usdm_design;
mod usdm_geography;
mod usdm_json_schema;
mod usdm_objects;
mod usdm_population_columns;
mod usdm_product;
mod usdm_references;
mod usdm_row_builders;
mod usdm_timeline;
mod usdm_values;
mod xpt;

pub use csv_dataset::load_csv_dataset;
pub(crate) use csv_dataset::parse_csv_bool;
pub(crate) use csv_records::{
    csv_records_to_dict_rows, normalize_dataset_name, normalize_metadata_name, read_csv_dict_rows,
    read_csv_records, row_string, CsvRecords,
};
pub use dataset_joins::{
    anti_join_dataset_on, inner_join_dataset_on, left_join_dataset, left_join_dataset_on,
    semi_join_dataset_on,
};
pub use dataset_operations::{
    dataset_column_values, deduplicate_dataset_by_columns, derive_column_from_column,
    derive_column_from_values, derive_literal_column, drop_dataset_columns, filter_dataset_by_mask,
    group_count_dataset, group_distinct_values_dataset, group_stat_dataset, rename_dataset_columns,
    row_number_dataset, select_dataset_columns,
};
pub use dataset_package::load_dataset_package_json;
pub(crate) use dataset_paths::{
    canonical_or_original, column_names, extension, file_name, file_stem, file_stem_str,
};
pub use dataset_transforms::sort_dataset_by_columns;
pub(crate) use json_table::records_to_frame;
use json_table::{json_rows_dataset, series_from_json_values};
pub use json_table::{metadata_row_dataset, metadata_rows_dataset};
pub use open_rules_data_dir::{load_open_rules_data_dir, load_open_rules_data_dir_with_warnings};
pub(crate) use row_key::{row_key, RowKeyValue};
use usdm_abbreviations::collect_usdm_abbreviation_rows;
use usdm_collectors::{
    collect_usdm_address_rows, collect_usdm_duration_rows, collect_usdm_person_name_rows,
    collect_usdm_range_rows,
};
use usdm_content::{
    collect_usdm_document_content_reference_rows, collect_usdm_narrative_content_item_rows,
    collect_usdm_narrative_content_rows, collect_usdm_scheduled_instance_rows,
    collect_usdm_timeline_rows,
};
use usdm_design::{
    collect_usdm_design_list_duplicate_rows, collect_usdm_design_rows,
    collect_usdm_interventional_design_rows,
};
use usdm_geography::{collect_usdm_geographic_scope_rows, collect_usdm_governance_date_rows};
use usdm_json_schema::collect_usdm_json_schema_issue_rows;
use usdm_objects::{apply_usdm_object_duplicate_flags, collect_usdm_object_rows};
use usdm_population_columns::{insert_planned_sex_columns, insert_quantity_columns};
use usdm_product::{
    collect_usdm_administrable_product_rows, collect_usdm_administration_rows,
    collect_usdm_amendment_reason_rows, collect_usdm_product_organization_role_rows,
    collect_usdm_strength_rows,
};
use usdm_references::{
    collect_usdm_id_instance_types, collect_usdm_reference_keys, parameter_map_reference_invalid,
    usdm_tag_references,
};
use usdm_timeline::format_usdm_id_name;
use usdm_values::{
    format_code, format_semicolon_list, format_string_list, json_string, string_array,
    value_exists, value_string,
};
pub use xpt::load_xpt_dataset;
#[cfg(test)]
pub(crate) use xpt::{XptVariableType, XPT_CARD_LEN, XPT_MAX_FILE_BYTES, XPT_NAMESTR_LEN};

pub type Result<T> = std::result::Result<T, DataError>;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("unsupported dataset file extension: {0}")]
    UnsupportedExtension(String),
    #[error("failed to read file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse dataset JSON {path}: {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to parse dataset CSV {path}: {source}")]
    CsvParse {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },
    #[error("failed to load dataset with Polars {path}: {source}")]
    Polars {
        path: PathBuf,
        #[source]
        source: PolarsError,
    },
    #[error("invalid dataset package: {0}")]
    InvalidDatasetPackage(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatasetSourceFormat {
    Csv,
    DatasetPackageJson,
    Xpt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetVariable {
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "type")]
    pub variable_type: Option<String>,
    #[serde(default)]
    pub length: Option<usize>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetMetadata {
    pub name: String,
    pub domain: Option<String>,
    pub label: Option<String>,
    pub filename: String,
    pub full_path: PathBuf,
    pub source_format: DatasetSourceFormat,
    pub variables: Vec<DatasetVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetSummary {
    pub name: String,
    pub domain: Option<String>,
    pub label: Option<String>,
    pub filename: String,
    pub full_path: PathBuf,
    pub columns: Vec<String>,
    pub row_count: usize,
}

#[derive(Debug, Clone)]
pub struct LoadedDataset {
    pub metadata: DatasetMetadata,
    frame: DataFrame,
}

impl LoadedDataset {
    pub fn new(metadata: DatasetMetadata, frame: DataFrame) -> Self {
        Self { metadata, frame }
    }

    pub fn metadata(&self) -> &DatasetMetadata {
        &self.metadata
    }

    pub fn summary(&self) -> DatasetSummary {
        DatasetSummary {
            name: self.metadata.name.clone(),
            domain: self.metadata.domain.clone(),
            label: self.metadata.label.clone(),
            filename: self.metadata.filename.clone(),
            full_path: self.metadata.full_path.clone(),
            columns: column_names(&self.frame),
            row_count: self.frame.height(),
        }
    }

    pub fn frame(&self) -> &DataFrame {
        &self.frame
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadDataWarning {
    pub path: PathBuf,
    pub kind: LoadDataWarningKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadDataWarningKind {
    UnsupportedExtension(String),
    InvalidNumericValue {
        dataset: String,
        variable: String,
        value: String,
        row: usize,
    },
    DeclaredVariableMissing {
        dataset: String,
        variable: String,
    },
    UndeclaredCsvColumn {
        dataset: String,
        variable: String,
    },
}

#[derive(Debug, Clone)]
pub struct LoadDataResult {
    pub datasets: Vec<LoadedDataset>,
    pub warnings: Vec<LoadDataWarning>,
}

pub fn load_dataset_file(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>> {
    let path = path.as_ref();
    match extension(path).as_deref() {
        Some("csv") => Ok(vec![load_csv_dataset(path)?]),
        Some("json") => load_dataset_package_json(path),
        Some("xpt") => Ok(vec![load_xpt_dataset(path)?]),
        Some(other) => Err(DataError::UnsupportedExtension(other.to_owned())),
        None => Err(DataError::UnsupportedExtension(String::new())),
    }
}

pub fn load_datasets_from_paths(paths: &[PathBuf]) -> Result<Vec<LoadedDataset>> {
    Ok(load_datasets_from_paths_with_warnings(paths)?.datasets)
}

pub fn load_datasets_from_paths_with_warnings(paths: &[PathBuf]) -> Result<LoadDataResult> {
    let mut datasets = Vec::new();
    let mut warnings = Vec::new();

    for path in paths {
        if path.is_file() {
            if is_supported_dataset_file(path) {
                datasets.extend(load_dataset_file(path)?);
            } else {
                warnings.push(unsupported_extension_warning(path));
            }
        } else if path.is_dir() {
            let mut entries = fs::read_dir(path)
                .map_err(|source| DataError::Io {
                    path: path.clone(),
                    source,
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|source| DataError::Io {
                    path: path.clone(),
                    source,
                })?;

            entries.sort_by_key(|entry| entry.path());

            for entry in entries {
                let child = entry.path();
                if !child.is_file() {
                    continue;
                }

                if is_supported_dataset_file(&child) {
                    datasets.extend(load_dataset_file(&child)?);
                } else {
                    warnings.push(unsupported_extension_warning(&child));
                }
            }
        } else {
            return Err(DataError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "path not found"),
            });
        }
    }

    Ok(LoadDataResult { datasets, warnings })
}

fn load_open_rules_json_data_dir(data_dir: &Path) -> Result<LoadDataResult> {
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
    if !population_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDesignPopulation",
            "usdm-population.json",
            &population_rows,
        )?);
    }
    if !role_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyRole",
            "usdm-study-role.json",
            &role_rows,
        )?);
    }
    if !role_blinding_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyRoleBlinding",
            "usdm-study-role-blinding.json",
            &role_blinding_rows,
        )?);
    }
    if !design_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDesign",
            "usdm-study-design.json",
            &design_rows,
        )?);
    }
    if !interventional_design_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "InterventionalStudyDesign",
            "usdm-interventional-study-design.json",
            &interventional_design_rows,
        )?);
    }
    if !design_characteristic_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDesignCharacteristicDuplicate",
            "usdm-study-design-characteristic-duplicate.json",
            &design_characteristic_rows,
        )?);
    }
    if !design_sub_type_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDesignSubTypeDuplicate",
            "usdm-study-design-sub-type-duplicate.json",
            &design_sub_type_rows,
        )?);
    }
    if !design_therapeutic_area_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDesignTherapeuticAreaDuplicate",
            "usdm-study-design-therapeutic-area-duplicate.json",
            &design_therapeutic_area_rows,
        )?);
    }
    if !version_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyVersion",
            "usdm-study-version.json",
            &version_rows,
        )?);
    }
    if !activity_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Activity",
            "usdm-activity.json",
            &activity_rows,
        )?);
    }
    if !duration_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Duration",
            "usdm-duration.json",
            &duration_rows,
        )?);
    }
    if !range_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Range",
            "usdm-range.json",
            &range_rows,
        )?);
    }
    if !person_name_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "PersonName",
            "usdm-person-name.json",
            &person_name_rows,
        )?);
    }
    if !address_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Address",
            "usdm-address.json",
            &address_rows,
        )?);
    }
    if !administrable_product_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "AdministrableProduct",
            "usdm-administrable-product.json",
            &administrable_product_rows,
        )?);
    }
    if !administration_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Administration",
            "usdm-administration.json",
            &administration_rows,
        )?);
    }
    if !strength_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Strength",
            "usdm-strength.json",
            &strength_rows,
        )?);
    }
    if !amendment_reason_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyAmendmentReason",
            "usdm-study-amendment-reason.json",
            &amendment_reason_rows,
        )?);
    }
    if !product_organization_role_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "ProductOrganizationRole",
            "usdm-product-organization-role.json",
            &product_organization_role_rows,
        )?);
    }
    if !biomedical_concept_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "BiomedicalConcept",
            "usdm-biomedical-concept.json",
            &biomedical_concept_rows,
        )?);
    }
    if !procedure_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Procedure",
            "usdm-procedure.json",
            &procedure_rows,
        )?);
    }
    if !subject_enrollment_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "SubjectEnrollment",
            "usdm-subject-enrollment.json",
            &subject_enrollment_rows,
        )?);
    }
    if !document_version_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyDefinitionDocumentVersion",
            "usdm-study-definition-document-version.json",
            &document_version_rows,
        )?);
    }
    if !substance_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Substance",
            "usdm-substance.json",
            &substance_rows,
        )?);
    }
    if !eligibility_criterion_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "EligibilityCriterion",
            "usdm-eligibility-criterion.json",
            &eligibility_criterion_rows,
        )?);
    }
    if !eligibility_criterion_item_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "EligibilityCriterionItem",
            "usdm-eligibility-criterion-item.json",
            &eligibility_criterion_item_rows,
        )?);
    }
    if !biospecimen_retention_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "BiospecimenRetention",
            "usdm-biospecimen-retention.json",
            &biospecimen_retention_rows,
        )?);
    }
    if !study_element_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyElement",
            "usdm-study-element.json",
            &study_element_rows,
        )?);
    }
    if !study_arm_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyArm",
            "usdm-study-arm.json",
            &study_arm_rows,
        )?);
    }
    if !cohort_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyCohort",
            "usdm-study-cohort.json",
            &cohort_rows,
        )?);
    }
    if !study_cell_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "StudyCell",
            "usdm-study-cell.json",
            &study_cell_rows,
        )?);
    }
    if !condition_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Condition",
            "usdm-condition.json",
            &condition_rows,
        )?);
    }
    if !parameter_map_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "ParameterMap",
            "usdm-parameter-map.json",
            &parameter_map_rows,
        )?);
    }
    if !syntax_template_text_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "SyntaxTemplateText",
            "usdm-syntax-template-text.json",
            &syntax_template_text_rows,
        )?);
    }
    if !narrative_content_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "NarrativeContent",
            "usdm-narrative-content.json",
            &narrative_content_rows,
        )?);
    }
    if !narrative_content_item_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "NarrativeContentItem",
            "usdm-narrative-content-item.json",
            &narrative_content_item_rows,
        )?);
    }
    if !abbreviation_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "Abbreviation",
            "usdm-abbreviation.json",
            &abbreviation_rows,
        )?);
    }
    if !object_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "USDMObject",
            "usdm-object.json",
            &object_rows,
        )?);
    }
    if !geographic_scope_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "GeographicScope",
            "usdm-geographic-scope.json",
            &geographic_scope_rows,
        )?);
    }
    if !governance_date_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "GovernanceDate",
            "usdm-governance-date.json",
            &governance_date_rows,
        )?);
    }
    if !document_content_reference_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "DocumentContentReference",
            "usdm-document-content-reference.json",
            &document_content_reference_rows,
        )?);
    }
    if !timeline_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "ScheduleTimeline",
            "usdm-schedule-timeline.json",
            &timeline_rows,
        )?);
    }
    if !scheduled_instance_rows.is_empty() {
        datasets.push(json_rows_dataset(
            data_dir,
            "ScheduledActivityInstance",
            "usdm-scheduled-activity-instance.json",
            &scheduled_instance_rows,
        )?);
    }
    for entity in [
        "StudyIdentifier",
        "ReferenceIdentifier",
        "AdministrableProductIdentifier",
        "MedicalDeviceIdentifier",
    ] {
        let rows = identifier_rows
            .iter()
            .filter(|row| {
                row.get("instanceType")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == entity)
            })
            .cloned()
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            datasets.push(json_rows_dataset(
                data_dir,
                entity,
                &format!(
                    "usdm-{}.json",
                    normalize_dataset_name(entity).to_ascii_lowercase()
                ),
                &rows,
            )?);
        }
    }
    datasets.push(json_rows_dataset(
        data_dir,
        "JSONSchemaIssue",
        "usdm-json-schema-issue.json",
        &json_schema_issue_rows,
    )?);

    if datasets.is_empty() {
        return Ok(LoadDataResult {
            datasets: Vec::new(),
            warnings: Vec::new(),
        });
    }

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

fn collect_usdm_cohort_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_study_cell_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_condition_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let id_types = collect_usdm_id_instance_types(value);
    collect_usdm_condition_rows_at(value, "", &id_types, rows);
}

fn collect_usdm_parameter_map_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let reference_keys = collect_usdm_reference_keys(value);
    collect_usdm_parameter_map_rows_at(value, "", &reference_keys, rows);
}

fn collect_usdm_syntax_template_text_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_identifier_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

pub(crate) fn duplicate_strings(values: &[String]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for value in values {
        *counts.entry(value.as_str()).or_insert(0) += 1;
    }
    let mut duplicates = values
        .iter()
        .filter(|value| counts.get(value.as_str()).is_some_and(|count| *count > 1))
        .cloned()
        .collect::<Vec<_>>();
    duplicates.sort();
    duplicates
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

pub(crate) fn named_usdm_object_name(values: &[Value], id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    values
        .iter()
        .find(|value| value_string(value.get("id").unwrap_or(&Value::Null)).as_deref() == Some(id))
        .and_then(|value| value_string(value.get("name").unwrap_or(&Value::Null)))
}

fn collect_direct_ids(value: Option<&Value>) -> HashSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("id").and_then(value_string))
        .collect()
}

fn collect_nested_ids(value: &Value, key: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_nested_ids_at(value, key, &mut ids);
    ids
}

fn collect_nested_ids_at(value: &Value, key: &str, ids: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(values) = object.get(key).and_then(Value::as_array) {
                ids.extend(
                    values
                        .iter()
                        .filter_map(|value| value.get("id").and_then(value_string)),
                );
            }
            for child in object.values() {
                collect_nested_ids_at(child, key, ids);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_nested_ids_at(child, key, ids);
            }
        }
        _ => {}
    }
}

fn collect_managed_site_ids(version: &Value) -> HashSet<String> {
    version
        .get("organizations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|organization| {
            organization
                .get("managedSites")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|site| site.get("id").and_then(value_string))
        })
        .collect()
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

fn is_supported_dataset_file(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("csv" | "json" | "xpt"))
}

fn unsupported_extension_warning(path: &Path) -> LoadDataWarning {
    LoadDataWarning {
        path: path.to_path_buf(),
        kind: LoadDataWarningKind::UnsupportedExtension(extension(path).unwrap_or_default()),
    }
}

pub fn dataset_names(datasets: &[LoadedDataset]) -> BTreeSet<String> {
    datasets
        .iter()
        .map(|dataset| dataset.metadata.name.clone())
        .collect()
}

fn number_value(value: f64) -> Value {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        return Value::Number(serde_json::Number::from(value as i64));
    }
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub(crate) fn take_dataset_rows(dataset: &LoadedDataset, indices: &[u32]) -> Result<LoadedDataset> {
    let indices = UInt32Chunked::from_vec("row_index".into(), indices.to_vec());
    let frame = dataset
        .frame
        .take(&indices)
        .map_err(|source| DataError::Polars {
            path: dataset.metadata.full_path.clone(),
            source,
        })?;
    Ok(LoadedDataset::new(dataset.metadata.clone(), frame))
}

fn cell_to_json_value(frame: &DataFrame, column_name: &str, row: usize) -> Result<Value> {
    let column = frame
        .column(column_name)
        .map_err(|source| DataError::Polars {
            path: PathBuf::from(column_name),
            source,
        })?;
    let value = column.get(row).map_err(|source| DataError::Polars {
        path: PathBuf::from(column_name),
        source,
    })?;
    if value.is_null() {
        return Ok(Value::Null);
    }
    if let Some(value) = value.extract_bool() {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = value.extract_str() {
        return Ok(Value::String(value.to_owned()));
    }
    match value {
        AnyValue::Float64(value) => return Ok(number_value(value)),
        AnyValue::Float32(value) => return Ok(number_value(value as f64)),
        _ => {}
    }
    if let Some(value) = value.extract::<i64>() {
        return Ok(Value::Number(serde_json::Number::from(value)));
    }
    if let Some(value) = value.extract::<u64>() {
        return Ok(Value::Number(serde_json::Number::from(value)));
    }
    if let Some(value) = value.extract::<f64>() {
        return Ok(number_value(value));
    }
    Ok(Value::String(value.to_string()))
}

fn cell_to_string(frame: &DataFrame, column_name: &str, row: usize) -> Result<Option<String>> {
    let column = frame
        .column(column_name)
        .map_err(|source| DataError::Polars {
            path: PathBuf::from(column_name),
            source,
        })?;
    let value = column.get(row).map_err(|source| DataError::Polars {
        path: PathBuf::from(column_name),
        source,
    })?;
    if value.is_null() {
        Ok(None)
    } else if let Some(value) = value.extract_str() {
        Ok(Some(value.to_owned()))
    } else {
        Ok(Some(value.to_string()))
    }
}

#[cfg(test)]
mod tests;
