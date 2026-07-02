#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

mod dataset_transforms;
mod open_rules_data_dir;
mod open_rules_variables;
mod usdm_json_schema;
mod usdm_references;

pub use dataset_transforms::sort_dataset_by_columns;
pub use open_rules_data_dir::{load_open_rules_data_dir, load_open_rules_data_dir_with_warnings};
use usdm_json_schema::collect_usdm_json_schema_issue_rows;
use usdm_references::{
    collect_usdm_id_instance_types, collect_usdm_reference_keys, parameter_map_reference_invalid,
    usdm_ref_references, usdm_tag_references,
};

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

pub fn metadata_row_dataset(
    source: &LoadedDataset,
    values: &BTreeMap<String, Value>,
) -> Result<LoadedDataset> {
    let columns = values
        .iter()
        .map(|(name, value)| series_from_json_values(name, std::slice::from_ref(value)).into())
        .collect::<Vec<_>>();
    let frame = DataFrame::new(1, columns).map_err(|source_error| DataError::Polars {
        path: source.metadata.full_path.clone(),
        source: source_error,
    })?;

    Ok(LoadedDataset::new(source.metadata.clone(), frame))
}

pub fn metadata_rows_dataset(
    source: &LoadedDataset,
    rows: &[BTreeMap<String, Value>],
) -> Result<LoadedDataset> {
    let names = rows
        .iter()
        .flat_map(|row| row.keys().cloned())
        .collect::<BTreeSet<_>>();
    let columns = names
        .iter()
        .map(|name| {
            let values = rows
                .iter()
                .map(|row| row.get(name).cloned().unwrap_or(Value::Null))
                .collect::<Vec<_>>();
            series_from_json_values(name, &values).into()
        })
        .collect::<Vec<_>>();
    let frame = DataFrame::new(rows.len(), columns).map_err(|source_error| DataError::Polars {
        path: source.metadata.full_path.clone(),
        source: source_error,
    })?;

    Ok(LoadedDataset::new(source.metadata.clone(), frame))
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

fn json_rows_dataset(
    data_dir: &Path,
    name: &str,
    filename: &str,
    rows: &[BTreeMap<String, Value>],
) -> Result<LoadedDataset> {
    let columns = if name == "JSONSchemaIssue" && rows.is_empty() {
        json_schema_issue_columns()
    } else {
        rows_to_columns(rows)
    };
    let frame = records_to_frame(&columns).map_err(|source| DataError::Polars {
        path: data_dir.to_path_buf(),
        source,
    })?;
    let variables = column_names(&frame)
        .into_iter()
        .map(|name| DatasetVariable {
            name,
            label: None,
            variable_type: None,
            length: None,
            extra: BTreeMap::new(),
        })
        .collect();
    let metadata = DatasetMetadata {
        name: name.to_owned(),
        domain: Some(name.to_owned()),
        label: None,
        filename: filename.to_owned(),
        full_path: canonical_or_original(data_dir),
        source_format: DatasetSourceFormat::DatasetPackageJson,
        variables,
    };

    Ok(LoadedDataset::new(metadata, frame))
}

fn json_schema_issue_columns() -> IndexMap<String, Vec<Value>> {
    ["path", "validator", "error_attribute", "message"]
        .into_iter()
        .map(|name| (name.to_owned(), Vec::new()))
        .collect()
}

fn rows_to_columns(rows: &[BTreeMap<String, Value>]) -> IndexMap<String, Vec<Value>> {
    let mut names = BTreeSet::new();
    for row in rows {
        names.extend(row.keys().cloned());
    }

    names
        .into_iter()
        .map(|name| {
            let values = rows
                .iter()
                .map(|row| row.get(&name).cloned().unwrap_or(Value::Null))
                .collect();
            (name, values)
        })
        .collect()
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

fn collect_usdm_design_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_design_list_duplicate_rows(
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

fn collect_usdm_interventional_design_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_duration_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_usdm_duration_rows_at(value, "", rows);
}

fn collect_usdm_duration_rows_at(
    value: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if object
                .get("instanceType")
                .and_then(Value::as_str)
                .is_some_and(|instance_type| instance_type == "Duration")
            {
                rows.push(usdm_duration_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_duration_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_duration_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn collect_usdm_range_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_usdm_range_rows_at(value, "", rows);
}

fn collect_usdm_range_rows_at(value: &Value, path: &str, rows: &mut Vec<BTreeMap<String, Value>>) {
    match value {
        Value::Object(object) => {
            if object
                .get("instanceType")
                .and_then(Value::as_str)
                .is_some_and(|instance_type| instance_type == "Range")
            {
                rows.push(usdm_range_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_range_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_range_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn collect_usdm_person_name_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_usdm_person_name_rows_at(value, "", rows);
}

fn collect_usdm_person_name_rows_at(
    value: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if object
                .get("instanceType")
                .and_then(Value::as_str)
                .is_some_and(|instance_type| instance_type == "PersonName")
            {
                rows.push(usdm_person_name_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_person_name_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_person_name_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn collect_usdm_address_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (org_index, organization) in version
            .get("organizations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(address) = organization.get("legalAddress") {
                rows.push(usdm_address_row(
                    address,
                    &format!(
                        "/study/versions/{version_index}/organizations/{org_index}/legalAddress"
                    ),
                    organization,
                ));
            }
        }
    }
}

fn collect_usdm_administrable_product_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let administered_product_ids = version
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|intervention| {
                intervention
                    .get("administrations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|administration| {
                administration
                    .get("administrableProductId")
                    .and_then(value_string)
            })
            .collect::<HashSet<_>>();
        let medical_devices = version
            .get("medicalDevices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (product_index, product) in version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            rows.push(usdm_administrable_product_row(
                product,
                &administered_product_ids,
                &medical_devices,
                &format!("/study/versions/{version_index}/administrableProducts/{product_index}"),
            ));
        }
    }
}

fn collect_usdm_administration_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        let administrable_products = version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let medical_devices = version
            .get("medicalDevices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (intervention_index, intervention) in version
            .get("studyInterventions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            for (administration_index, administration) in intervention
                .get("administrations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                rows.push(usdm_administration_row(
                    administration,
                    intervention,
                    &administrable_products,
                    &medical_devices,
                    &format!(
                        "/study/versions/{version_index}/studyInterventions/{intervention_index}/administrations/{administration_index}"
                    ),
                ));
            }
        }
    }
}

fn collect_usdm_strength_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
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
                    collect_usdm_strength_rows_at(
                        substance,
                        product,
                        ingredient,
                        substance,
                        &format!(
                            "/study/versions/{version_index}/administrableProducts/{product_index}/ingredients/{ingredient_index}/substance"
                        ),
                        rows,
                    );
                }
            }
        }
    }
}

fn collect_usdm_strength_rows_at(
    value: &Value,
    product: &Value,
    ingredient: &Value,
    substance: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if let Some(strengths) = object.get("strengths").and_then(Value::as_array) {
                for (strength_index, strength) in strengths.iter().enumerate() {
                    rows.push(usdm_strength_row(
                        strength,
                        product,
                        ingredient,
                        substance,
                        &format!("{path}/strengths/{strength_index}"),
                    ));
                }
            }
            for (key, child) in object {
                if key != "strengths" {
                    collect_usdm_strength_rows_at(
                        child,
                        product,
                        ingredient,
                        substance,
                        &format!("{path}/{key}"),
                        rows,
                    );
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_strength_rows_at(
                    child,
                    product,
                    ingredient,
                    substance,
                    &format!("{path}/{index}"),
                    rows,
                );
            }
        }
        _ => {}
    }
}

fn collect_usdm_amendment_reason_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for (version_index, version) in versions.iter().enumerate() {
        for (amendment_index, amendment) in version
            .get("amendments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let primary_code = amendment
                .get("primaryReason")
                .and_then(|reason| reason.get("code"))
                .and_then(|code| code.get("code"))
                .and_then(value_string);
            if let Some(reason) = amendment.get("primaryReason") {
                rows.push(usdm_amendment_reason_row(
                    reason,
                    &format!(
                        "/study/versions/{version_index}/amendments/{amendment_index}/primaryReason"
                    ),
                    amendment,
                    None,
                ));
            }
            for (reason_index, reason) in amendment
                .get("secondaryReasons")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                rows.push(usdm_amendment_reason_row(
                    reason,
                    &format!(
                        "/study/versions/{version_index}/amendments/{amendment_index}/secondaryReasons/{reason_index}"
                    ),
                    amendment,
                    primary_code.as_deref(),
                ));
            }
        }
    }
}

fn collect_usdm_product_organization_role_rows(
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
        let valid_ids = version
            .get("administrableProducts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                version
                    .get("medicalDevices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .filter_map(|value| value.get("id").and_then(value_string))
            .collect::<HashSet<_>>();
        for (role_index, role) in version
            .get("productOrganizationRoles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            rows.push(usdm_product_organization_role_row(
                role,
                &format!("/study/versions/{version_index}/productOrganizationRoles/{role_index}"),
                &valid_ids,
            ));
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

fn collect_usdm_narrative_content_item_rows(
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

fn collect_usdm_narrative_content_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_abbreviation_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(abbreviations) = version.get("abbreviations").and_then(Value::as_array) else {
            continue;
        };
        let mut version_rows = abbreviations
            .iter()
            .enumerate()
            .map(|(abbreviation_index, abbreviation)| {
                usdm_abbreviation_row(abbreviation, version, version_index, abbreviation_index)
            })
            .collect::<Vec<_>>();
        apply_abbreviation_duplicate_flags(&mut version_rows);
        rows.extend(version_rows);
    }
}

fn collect_usdm_object_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
    collect_usdm_object_rows_at(value, "", rows);
}

fn collect_usdm_object_rows_at(value: &Value, path: &str, rows: &mut Vec<BTreeMap<String, Value>>) {
    match value {
        Value::Object(object) => {
            if object.contains_key("id") {
                rows.push(usdm_object_row(value, path));
            }
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_object_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_object_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn collect_usdm_geographic_scope_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_governance_date_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_document_content_reference_rows(
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

fn collect_usdm_timeline_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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

fn collect_usdm_scheduled_instance_rows(value: &Value, rows: &mut Vec<BTreeMap<String, Value>>) {
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
        let epoch_parent_designs = usdm_child_parent_designs(study_designs, "epochs");
        let encounter_parent_designs = usdm_child_parent_designs(study_designs, "encounters");
        for (design_index, design) in study_designs.iter().enumerate() {
            let Some(timelines) = design.get("scheduleTimelines").and_then(Value::as_array) else {
                continue;
            };
            for (timeline_index, timeline) in timelines.iter().enumerate() {
                let Some(instances) = timeline.get("instances").and_then(Value::as_array) else {
                    continue;
                };
                for (instance_index, instance) in instances.iter().enumerate() {
                    rows.push(usdm_scheduled_instance_row(
                        instance,
                        design,
                        &epoch_parent_designs,
                        &encounter_parent_designs,
                        &format!(
                            "/study/versions/{version_index}/studyDesigns/{design_index}/scheduleTimelines/{timeline_index}/instances/{instance_index}"
                        ),
                    ));
                }
            }
        }
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

fn usdm_duration_row(duration: &Value, path: &str) -> BTreeMap<String, Value> {
    let text = duration.get("text").and_then(value_string);
    let quantity = duration.get("quantity");
    let quantity_present = quantity.is_some_and(|value| !value.is_null());
    let text_present = text.as_ref().is_some_and(|value| !value.is_empty());
    let duration_will_vary = duration.get("durationWillVary").and_then(Value::as_bool);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(duration.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(duration.get("id")));
    row.insert(
        "text".to_owned(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "quantity".to_owned(),
        quantity
            .map(format_quantity_value)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "quantity(value/range)".to_owned(),
        quantity
            .map(format_quantity_value)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "durationWillVary".to_owned(),
        duration_will_vary.map(Value::Bool).unwrap_or(Value::Null),
    );
    row.insert(
        "duration_missing_text_and_quantity".to_owned(),
        Value::Bool(!text_present && !quantity_present),
    );
    row.insert(
        "duration_vary_quantity_conflict".to_owned(),
        Value::Bool(match duration_will_vary {
            Some(true) => quantity_present,
            Some(false) => !quantity_present,
            None => false,
        }),
    );
    row
}

fn usdm_range_row(range: &Value, path: &str) -> BTreeMap<String, Value> {
    let min = range.get("minValue");
    let max = range.get("maxValue");
    let min_value = min
        .and_then(|value| value.get("value"))
        .and_then(Value::as_f64);
    let max_value = max
        .and_then(|value| value.get("value"))
        .and_then(Value::as_f64);
    let min_unit = quantity_unit_code(min);
    let max_unit = quantity_unit_code(max);
    let same_or_missing_units = min_unit == max_unit;
    let unit_xor = min_unit.is_some() ^ max_unit.is_some();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(range.get("instanceType")),
    );
    row.insert(
        "minValue".to_owned(),
        min.map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "maxValue".to_owned(),
        max.map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "range_min_not_less_than_max".to_owned(),
        Value::Bool(
            same_or_missing_units
                && min_value
                    .zip(max_value)
                    .is_some_and(|(min_value, max_value)| min_value >= max_value),
        ),
    );
    row.insert("range_unit_xor".to_owned(), Value::Bool(unit_xor));
    row
}

fn usdm_person_name_row(person_name: &Value, path: &str) -> BTreeMap<String, Value> {
    let family_name = person_name.get("familyName").and_then(value_string);
    let text = person_name.get("text").and_then(value_string);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(person_name.get("instanceType")),
    );
    row.insert(
        "familyName".to_owned(),
        family_name.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "text".to_owned(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "person_name_missing_text_and_family_name".to_owned(),
        Value::Bool(
            person_name
                .get("familyName")
                .and_then(value_string)
                .is_none_or(|value| value.is_empty())
                && person_name
                    .get("text")
                    .and_then(value_string)
                    .is_none_or(|value| value.is_empty()),
        ),
    );
    row
}

fn usdm_address_row(address: &Value, path: &str, organization: &Value) -> BTreeMap<String, Value> {
    let fields = [
        "text",
        "lines",
        "city",
        "district",
        "state",
        "postalCode",
        "country",
    ];
    let all_missing = fields
        .iter()
        .all(|field| address.get(*field).is_none_or(usdm_address_field_is_blank));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "Organization.id".to_owned(),
        json_string(organization.get("id")),
    );
    row.insert(
        "Organization.name".to_owned(),
        json_string(organization.get("name")),
    );
    for field in fields {
        row.insert(field.to_owned(), jsonata_exists_rep(address.get(field)));
    }
    row.insert("address_all_blank".to_owned(), Value::Bool(all_missing));
    row
}

fn usdm_address_field_is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.is_empty(),
        Value::Object(_) => false,
        _ => value_string(value).is_none_or(|value| value.is_empty()),
    }
}

fn jsonata_exists_rep(value: Option<&Value>) -> Value {
    match value {
        None => Value::String("Missing".to_owned()),
        Some(Value::Null) => Value::Null,
        Some(Value::Array(values)) => {
            let values: Vec<String> = values.iter().filter_map(value_string).collect();
            Value::String(format_string_list(&values))
        }
        Some(value) => value_string(value)
            .map(Value::String)
            .unwrap_or_else(|| Value::String(value.to_string())),
    }
}

fn usdm_administrable_product_row(
    product: &Value,
    administered_product_ids: &HashSet<String>,
    medical_devices: &[Value],
    path: &str,
) -> BTreeMap<String, Value> {
    let product_id = product.get("id").and_then(value_string).unwrap_or_default();
    let embedded_devices = medical_devices
        .iter()
        .filter(|device| {
            device
                .get("embeddedProductId")
                .and_then(value_string)
                .as_deref()
                == Some(&product_id)
        })
        .collect::<Vec<_>>();
    let has_sourcing = product
        .get("sourcing")
        .is_some_and(|sourcing| !sourcing.is_null());
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(product.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(product.get("id")));
    row.insert("name".to_owned(), json_string(product.get("name")));
    row.insert(
        "sourcing".to_owned(),
        product
            .get("sourcing")
            .map(|sourcing| Value::String(format_code(Some(sourcing))))
            .unwrap_or(Value::Null),
    );
    row.insert(
        "MedicalDevice.id".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("id").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "MedicalDevice.name".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("name").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "MedicalDevice.embeddedProductId".to_owned(),
        Value::String(format_semicolon_list(
            &embedded_devices
                .iter()
                .filter_map(|device| device.get("embeddedProductId").and_then(value_string))
                .collect::<Vec<_>>(),
        )),
    );
    row.insert(
        "administrable_product_embedded_only_sourcing".to_owned(),
        Value::Bool(
            has_sourcing
                && !embedded_devices.is_empty()
                && !administered_product_ids.contains(&product_id),
        ),
    );
    row
}

fn usdm_administration_row(
    administration: &Value,
    intervention: &Value,
    administrable_products: &[Value],
    medical_devices: &[Value],
    path: &str,
) -> BTreeMap<String, Value> {
    let dose = administration.get("dose");
    let route = administration.get("route");
    let frequency = administration.get("frequency");
    let dose_id_exists = value_exists(dose.and_then(|dose| dose.get("id")));
    let route_id_exists = value_exists(route.and_then(|route| route.get("id")));
    let frequency_id_exists = value_exists(frequency.and_then(|frequency| frequency.get("id")));
    let administrable_product_id = administration
        .get("administrableProductId")
        .and_then(value_string);
    let medical_device_id = administration.get("medicalDeviceId").and_then(value_string);
    let medical_device = medical_device_id.as_deref().and_then(|id| {
        medical_devices
            .iter()
            .find(|device| device.get("id").and_then(value_string).as_deref() == Some(id))
    });
    let embedded_product_id = medical_device
        .and_then(|device| device.get("embeddedProductId"))
        .and_then(value_string);
    let has_product = administrable_product_id
        .as_deref()
        .is_some_and(|id| !id.is_empty())
        || embedded_product_id
            .as_deref()
            .is_some_and(|id| !id.is_empty());
    let product_name = administrable_product_id
        .as_deref()
        .or(embedded_product_id.as_deref())
        .and_then(|id| named_usdm_object_name(administrable_products, id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "StudyIntervention.id".to_owned(),
        json_string(intervention.get("id")),
    );
    row.insert(
        "StudyIntervention.name".to_owned(),
        json_string(intervention.get("name")),
    );
    row.insert("name".to_owned(), json_string(administration.get("name")));
    row.insert(
        "dose.id".to_owned(),
        json_string(dose.and_then(|dose| dose.get("id"))),
    );
    row.insert(
        "dose(value)".to_owned(),
        dose.map(usdm_format_quantity_or_missing)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "dose(value/range)".to_owned(),
        dose.map(usdm_format_quantity_or_missing)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "route.id".to_owned(),
        json_string(route.and_then(|route| route.get("id"))),
    );
    row.insert(
        "route".to_owned(),
        route
            .and_then(|route| route.get("standardCode"))
            .map(|code| Value::String(format_code(Some(code))))
            .unwrap_or_else(|| Value::String("()".to_owned())),
    );
    row.insert(
        "frequency.id".to_owned(),
        json_string(frequency.and_then(|frequency| frequency.get("id"))),
    );
    row.insert(
        "frequency".to_owned(),
        frequency
            .and_then(|frequency| frequency.get("standardCode"))
            .map(|code| Value::String(format_code(Some(code))))
            .unwrap_or_else(|| Value::String("()".to_owned())),
    );
    row.insert(
        "administrableProductId".to_owned(),
        json_string(administration.get("administrableProductId")),
    );
    row.insert(
        "medicalDeviceId".to_owned(),
        json_string(administration.get("medicalDeviceId")),
    );
    row.insert(
        "MedicalDevice.name".to_owned(),
        medical_device
            .and_then(|device| device.get("name"))
            .and_then(value_string)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "MedicalDevice.embeddedProductId".to_owned(),
        embedded_product_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "AdministrableProduct.name".to_owned(),
        product_name.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "administration_dose_route_xor".to_owned(),
        Value::Bool(dose_id_exists != route_id_exists),
    );
    row.insert(
        "administration_dose_without_frequency".to_owned(),
        Value::Bool(dose_id_exists && !frequency_id_exists),
    );
    row.insert(
        "administration_dose_product_xor".to_owned(),
        Value::Bool(dose_id_exists != has_product),
    );
    row.insert(
        "administration_duplicate_embedded_product".to_owned(),
        Value::Bool(
            medical_device_id
                .as_deref()
                .is_some_and(|id| !id.is_empty())
                && administrable_product_id.is_some()
                && administrable_product_id == embedded_product_id,
        ),
    );
    row
}

fn usdm_strength_row(
    strength: &Value,
    product: &Value,
    ingredient: &Value,
    substance: &Value,
    path: &str,
) -> BTreeMap<String, Value> {
    let numerator = strength.get("numerator");
    let numerator_value = numerator.and_then(|value| value.get("value"));
    let numerator_min = numerator.and_then(|value| value.get("minValue"));
    let numerator_max = numerator.and_then(|value| value.get("maxValue"));
    let denominator = strength.get("denominator");
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
    row.insert("Substance.id".to_owned(), json_string(substance.get("id")));
    row.insert(
        "Substance.name".to_owned(),
        json_string(substance.get("name")),
    );
    row.insert("name".to_owned(), json_string(strength.get("name")));
    row.insert(
        "numerator.value".to_owned(),
        numerator_value.cloned().unwrap_or(Value::Null),
    );
    row.insert(
        "numerator.minValue".to_owned(),
        numerator_min
            .map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "numerator.maxValue".to_owned(),
        numerator_max
            .map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "denominator.id".to_owned(),
        json_string(denominator.and_then(|denominator| denominator.get("id"))),
    );
    row.insert(
        "denominator.value".to_owned(),
        denominator
            .and_then(|denominator| denominator.get("value"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    row.insert(
        "strength_numerator_value_missing_unit".to_owned(),
        Value::Bool(
            value_exists(numerator_value) && !value_exists(numerator.and_then(|n| n.get("unit"))),
        ),
    );
    row.insert(
        "strength_numerator_range_missing_unit".to_owned(),
        Value::Bool(
            (value_exists(numerator_min.and_then(|v| v.get("value")))
                && !value_exists(numerator_min.and_then(|v| v.get("unit"))))
                || (value_exists(numerator_max.and_then(|v| v.get("value")))
                    && !value_exists(numerator_max.and_then(|v| v.get("unit")))),
        ),
    );
    row.insert(
        "strength_denominator_missing_unit".to_owned(),
        Value::Bool(
            value_exists(denominator.and_then(|denominator| denominator.get("id")))
                && !value_exists(denominator.and_then(|denominator| denominator.get("unit"))),
        ),
    );
    row
}

fn usdm_amendment_reason_row(
    reason: &Value,
    path: &str,
    amendment: &Value,
    primary_code: Option<&str>,
) -> BTreeMap<String, Value> {
    let reason_code = reason
        .get("code")
        .and_then(|code| code.get("code"))
        .and_then(value_string);
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
    row.insert(
        "code".to_owned(),
        Value::String(format_code(reason.get("code"))),
    );
    row.insert(
        "primaryReason.code".to_owned(),
        primary_code
            .map(|code| Value::String(code.to_owned()))
            .unwrap_or_else(|| Value::String(format_code(reason.get("code")))),
    );
    row.insert(
        "primary_reason_not_applicable".to_owned(),
        Value::Bool(reason_code.as_deref() == Some("C48660")),
    );
    row.insert(
        "secondary_reason_matches_primary".to_owned(),
        Value::Bool(
            primary_code.is_some()
                && reason_code
                    .as_deref()
                    .is_some_and(|code| Some(code) == primary_code),
        ),
    );
    row
}

fn usdm_product_organization_role_row(
    role: &Value,
    path: &str,
    valid_ids: &HashSet<String>,
) -> BTreeMap<String, Value> {
    let applies_to_ids = string_array(role.get("appliesToIds"));
    let valid_reference =
        !applies_to_ids.is_empty() && applies_to_ids.iter().any(|id| valid_ids.contains(id));
    let invalid_reference = applies_to_ids.iter().any(|id| !valid_ids.contains(id));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("name".to_owned(), json_string(role.get("name")));
    row.insert(
        "appliesToIds".to_owned(),
        if applies_to_ids.is_empty() {
            Value::Null
        } else {
            Value::String(format_string_list(&applies_to_ids))
        },
    );
    row.insert(
        "product_role_missing_valid_target".to_owned(),
        Value::Bool(!valid_reference),
    );
    row.insert(
        "product_role_invalid_target".to_owned(),
        Value::Bool(invalid_reference),
    );
    row
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

fn usdm_child_parent_designs(
    study_designs: &[Value],
    child_collection: &str,
) -> HashMap<String, String> {
    let mut parents = HashMap::new();
    for design in study_designs {
        let design_id = design.get("id").and_then(value_string).unwrap_or_default();
        for child in design
            .get(child_collection)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = child.get("id").and_then(value_string) {
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

fn usdm_abbreviation_row(
    abbreviation: &Value,
    version: &Value,
    version_index: usize,
    abbreviation_index: usize,
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/abbreviations/{abbreviation_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(abbreviation.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(abbreviation.get("id")));
    row.insert("StudyVersion.id".to_owned(), json_string(version.get("id")));
    row.insert(
        "StudyVersion.versionIdentifier".to_owned(),
        json_string(version.get("versionIdentifier")),
    );
    row.insert(
        "abbreviatedText".to_owned(),
        json_string(abbreviation.get("abbreviatedText")),
    );
    row.insert(
        "expandedText".to_owned(),
        json_string(abbreviation.get("expandedText")),
    );
    row
}

fn apply_abbreviation_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut expanded_counts: HashMap<String, usize> = HashMap::new();
    let mut abbreviated_counts: HashMap<String, usize> = HashMap::new();
    for row in rows.iter() {
        let expanded = row
            .get("expandedText")
            .and_then(value_string)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !expanded.is_empty() {
            *expanded_counts.entry(expanded).or_insert(0) += 1;
        }
        let abbreviated = row
            .get("abbreviatedText")
            .and_then(value_string)
            .unwrap_or_default();
        if !abbreviated.is_empty() {
            *abbreviated_counts.entry(abbreviated).or_insert(0) += 1;
        }
    }
    for row in rows.iter_mut() {
        let expanded = row
            .get("expandedText")
            .and_then(value_string)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let expanded_duplicate = !expanded.is_empty()
            && expanded_counts
                .get(&expanded)
                .is_some_and(|count| *count > 1);
        let abbreviated = row
            .get("abbreviatedText")
            .and_then(value_string)
            .unwrap_or_default();
        let abbreviated_duplicate = !abbreviated.is_empty()
            && abbreviated_counts
                .get(&abbreviated)
                .is_some_and(|count| *count > 1);
        row.insert(
            "abbreviation_expanded_text_duplicate".to_owned(),
            Value::Bool(expanded_duplicate),
        );
        row.insert(
            "abbreviation_text_duplicate".to_owned(),
            Value::Bool(abbreviated_duplicate),
        );
    }
}

fn duplicate_strings(values: &[String]) -> Vec<String> {
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

fn usdm_object_row(object: &Value, path: &str) -> BTreeMap<String, Value> {
    let id = value_string(object.get("id").unwrap_or(&Value::Null)).unwrap_or_default();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(object.get("instanceType")),
    );
    row.insert("id".to_owned(), Value::String(id.clone()));
    row.insert("name".to_owned(), json_string(object.get("name")));
    row.insert(
        "usdm_id_contains_space".to_owned(),
        Value::Bool(id.contains(' ')),
    );
    row.insert(
        "usdm_duplicate_name_for_class".to_owned(),
        Value::Bool(false),
    );
    row.insert("usdm_duplicate_id".to_owned(), Value::Bool(false));
    row
}

fn apply_usdm_object_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut name_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for row in rows.iter() {
        let instance_type = row
            .get("instanceType")
            .and_then(value_string)
            .unwrap_or_default();
        let name = row.get("name").and_then(value_string).unwrap_or_default();
        if !instance_type.is_empty() && !name.is_empty() {
            *name_counts.entry((instance_type, name)).or_insert(0) += 1;
        }
        let id = row.get("id").and_then(value_string).unwrap_or_default();
        if !id.is_empty() {
            *id_counts.entry(id).or_insert(0) += 1;
        }
    }

    for row in rows.iter_mut() {
        let instance_type = row
            .get("instanceType")
            .and_then(value_string)
            .unwrap_or_default();
        let name = row.get("name").and_then(value_string).unwrap_or_default();
        let duplicate_name = !instance_type.is_empty()
            && !name.is_empty()
            && name_counts
                .get(&(instance_type, name))
                .is_some_and(|count| *count > 1);
        let id = row.get("id").and_then(value_string).unwrap_or_default();
        let duplicate_id = !id.is_empty() && id_counts.get(&id).is_some_and(|count| *count > 1);
        row.insert(
            "usdm_duplicate_name_for_class".to_owned(),
            Value::Bool(duplicate_name),
        );
        row.insert("usdm_duplicate_id".to_owned(), Value::Bool(duplicate_id));
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

fn usdm_scheduled_instance_row(
    instance: &Value,
    design: &Value,
    epoch_parent_designs: &HashMap<String, String>,
    encounter_parent_designs: &HashMap<String, String>,
    path: &str,
) -> BTreeMap<String, Value> {
    let design_id = design.get("id").and_then(value_string).unwrap_or_default();
    let epoch_id = instance.get("epochId").and_then(value_string);
    let encounter_id = instance.get("encounterId").and_then(value_string);
    let referenced_epoch_parent = epoch_id
        .as_deref()
        .and_then(|id| epoch_parent_designs.get(id))
        .cloned()
        .unwrap_or_else(|| "[Invalid epochId]".to_owned());
    let referenced_encounter_parent = encounter_id
        .as_deref()
        .and_then(|id| encounter_parent_designs.get(id))
        .cloned()
        .unwrap_or_else(|| "[Invalid encounterId]".to_owned());
    let mut row = BTreeMap::new();
    insert_study_design_context(&mut row, design);
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(instance.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(instance.get("id")));
    row.insert("name".to_owned(), json_string(instance.get("name")));
    row.insert(
        "epochId".to_owned(),
        epoch_id.clone().map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "encounterId".to_owned(),
        encounter_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "Referenced epoch's parent StudyDesign.id".to_owned(),
        Value::String(referenced_epoch_parent.clone()),
    );
    row.insert(
        "Referenced encounter's parent StudyDesign.id".to_owned(),
        Value::String(referenced_encounter_parent.clone()),
    );
    row.insert(
        "scheduled_instance_epoch_wrong_design".to_owned(),
        Value::Bool(
            epoch_id
                .as_deref()
                .is_some_and(|_| referenced_epoch_parent != design_id),
        ),
    );
    row.insert(
        "scheduled_instance_encounter_wrong_design".to_owned(),
        Value::Bool(
            encounter_id
                .as_deref()
                .is_some_and(|_| referenced_encounter_parent != design_id),
        ),
    );
    row
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

fn named_usdm_object_name(values: &[Value], id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    values
        .iter()
        .find(|value| value_string(value.get("id").unwrap_or(&Value::Null)).as_deref() == Some(id))
        .and_then(|value| value_string(value.get("name").unwrap_or(&Value::Null)))
}

fn insert_quantity_columns(
    row: &mut BTreeMap<String, Value>,
    name: &str,
    quantity: &Value,
    cohorts: &[Value],
) {
    row.insert(format!("{name}.id"), json_string(quantity.get("id")));
    row.insert(
        format!("{name}(value/range)"),
        Value::String(format_quantity(quantity)),
    );
    row.insert(
        format!("{name}.present"),
        Value::Bool(quantity_present(quantity)),
    );
    row.insert(
        format!("{name}.has_unit"),
        Value::Bool(quantity_has_unit(quantity)),
    );
    let cohort_present = cohorts
        .iter()
        .map(|cohort| cohort.get(name).is_some_and(quantity_present))
        .collect::<Vec<_>>();
    let cohort_missing = cohorts
        .iter()
        .map(|cohort| {
            !cohort
                .as_object()
                .is_some_and(|object| object.contains_key(name))
        })
        .collect::<Vec<_>>();
    row.insert(
        format!("cohorts.{name}.any_present"),
        Value::Bool(cohort_present.iter().any(|present| *present)),
    );
    row.insert(
        format!("cohorts.{name}.all_present"),
        Value::Bool(!cohorts.is_empty() && cohort_present.iter().all(|present| *present)),
    );
    row.insert(
        format!("cohorts.{name}.any_missing"),
        Value::Bool(cohort_missing.iter().any(|missing| *missing)),
    );
    row.insert(
        format!("cohorts.{name}.has_unit"),
        Value::Bool(
            cohorts
                .iter()
                .any(|cohort| cohort.get(name).is_some_and(quantity_has_unit)),
        ),
    );
    row.insert(
        "cohorts.name".to_owned(),
        Value::String(format_cohort_names(cohorts)),
    );
    row.insert(
        format!("cohorts.{name}.id"),
        Value::String(format_cohort_quantity_ids(cohorts, name)),
    );
    row.insert(
        format!("cohorts.{name}(value/range)"),
        Value::String(format_cohort_quantities(cohorts, name)),
    );
}

fn insert_planned_sex_columns(row: &mut BTreeMap<String, Value>, planned_sex: Option<&Value>) {
    let values = planned_sex
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    row.insert(
        "plannedSex".to_owned(),
        Value::String(format_planned_sex(&values)),
    );
    row.insert(
        "plannedSex.invalid".to_owned(),
        Value::Bool(planned_sex_invalid(&values)),
    );
}

fn json_string(value: Option<&Value>) -> Value {
    value
        .and_then(value_string)
        .map(Value::String)
        .unwrap_or(Value::Null)
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

fn value_exists(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
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

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(value_string).collect())
        .unwrap_or_default()
}

fn format_string_list(values: &[String]) -> String {
    format!("[{}]", values.join(", "))
}

fn format_semicolon_list(values: &[String]) -> String {
    format!("[{}]", values.join("; "))
}

fn format_usdm_id_name(value: &Value) -> Option<String> {
    let id = value.get("id").and_then(value_string)?;
    let name = value.get("name").and_then(value_string).unwrap_or_default();
    Some(format!("{id}: {name}"))
}

fn ordered_usdm_objects_by_previous_next(value: Option<&Value>) -> Vec<String> {
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

fn timeline_usdm_object_ref_order(
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

fn format_usdm_object_order(values: &[String]) -> String {
    format!("[ {} ]", values.join(" > "))
}

fn format_code(code: Option<&Value>) -> String {
    let Some(code) = code else {
        return String::new();
    };
    let decode = code
        .get("decode")
        .and_then(value_string)
        .unwrap_or_default();
    let code_value = code.get("code").and_then(value_string).unwrap_or_default();
    if code_value.is_empty() {
        decode
    } else {
        format!("{decode} ({code_value})")
    }
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

fn format_quantity_value(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            if object.contains_key("value") {
                return format_quantity_single(value);
            }
            if object.contains_key("minValue") || object.contains_key("maxValue") {
                let min = object
                    .get("minValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                let max = object
                    .get("maxValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                return format!("{min} to {max}");
            }
            value.to_string()
        }
        Value::Null => "Missing".to_owned(),
        _ => value_string(value).unwrap_or_else(|| value.to_string()),
    }
}

fn usdm_format_quantity_or_missing(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            if object.contains_key("value") {
                return format_quantity_single(value);
            }
            if object.contains_key("minValue") || object.contains_key("maxValue") {
                let min = object
                    .get("minValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                let max = object
                    .get("maxValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                return format!("{min} to {max}");
            }
            "Missing".to_owned()
        }
        Value::Null => "Missing".to_owned(),
        _ => value_string(value).unwrap_or_else(|| value.to_string()),
    }
}

fn format_quantity_single(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_default();
    if unit.is_empty() {
        quantity
    } else {
        format!("{quantity} {unit}")
    }
}

fn format_quantity_single_with_missing_unit(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_else(|| "unit not specified".to_owned());
    format!("{quantity} {unit}")
}

fn quantity_unit_code(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value.get("unit"))
        .and_then(|unit| unit.get("standardCode"))
        .and_then(|code| code.get("code"))
        .and_then(value_string)
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

fn format_planned_sex(values: &[Value]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| {
                format!(
                    "{}: {} ({})",
                    value_string(value.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(value.get("decode").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(value.get("code").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn format_timeline_names(timelines: &[&Value]) -> String {
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

fn planned_sex_invalid(values: &[Value]) -> bool {
    let codes = values
        .iter()
        .filter_map(|value| value.get("code").and_then(value_string))
        .collect::<Vec<_>>();
    let distinct = codes.iter().collect::<BTreeSet<_>>().len();
    codes.len() != distinct
        || codes
            .iter()
            .any(|code| !matches!(code.as_str(), "C16576" | "C20197"))
}

fn quantity_present(quantity: &Value) -> bool {
    !quantity.is_null()
}

fn quantity_has_unit(quantity: &Value) -> bool {
    quantity.get("unit").is_some_and(|unit| !unit.is_null())
        || quantity
            .get("minValue")
            .and_then(|value| value.get("unit"))
            .is_some_and(|unit| !unit.is_null())
        || quantity
            .get("maxValue")
            .and_then(|value| value.get("unit"))
            .is_some_and(|unit| !unit.is_null())
}

fn format_quantity(quantity: &Value) -> String {
    if quantity.is_null() {
        return "null".to_owned();
    }
    if let Some(value) = quantity.get("value").and_then(value_string) {
        return if quantity.get("unit").is_some_and(|unit| !unit.is_null()) {
            format!("{value} {}", format_unit(quantity.get("unit")))
        } else {
            value
        };
    }
    if let Some(min_value) = quantity.get("minValue") {
        let max_value = quantity.get("maxValue").unwrap_or(&Value::Null);
        let min = min_value
            .get("value")
            .and_then(value_string)
            .unwrap_or_default();
        let max = max_value
            .get("value")
            .and_then(value_string)
            .unwrap_or_default();
        return if quantity_has_unit(quantity) {
            format!(
                "{min} {} to {max} {}",
                format_unit(min_value.get("unit")),
                format_unit(max_value.get("unit"))
            )
        } else {
            format!("{min} to {max}")
        };
    }
    value_string(quantity).unwrap_or_else(|| "null".to_owned())
}

fn format_unit(unit: Option<&Value>) -> String {
    let Some(unit) = unit else {
        return String::new();
    };
    let decode = unit
        .get("standardCode")
        .and_then(|code| code.get("decode"))
        .and_then(value_string)
        .unwrap_or_default();
    let code = unit
        .get("standardCode")
        .and_then(|code| code.get("code"))
        .and_then(value_string)
        .unwrap_or_default();
    if code.is_empty() {
        decode
    } else {
        format!("{decode} ({code})")
    }
}

fn format_cohort_names(cohorts: &[Value]) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(cohort.get("name").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_cohort_quantity_ids(cohorts: &[Value], name: &str) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                let quantity = cohort.get(name).unwrap_or(&Value::Null);
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(quantity.get("id").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_cohort_quantities(cohorts: &[Value], name: &str) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    format_quantity(cohort.get(name).unwrap_or(&Value::Null))
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub fn load_csv_dataset(path: impl AsRef<Path>) -> Result<LoadedDataset> {
    let path = path.as_ref();
    let raw_frame = CsvReadOptions::default()
        .with_infer_schema_length(Some(0))
        .try_into_reader_with_file_path(Some(path.to_path_buf()))
        .map_err(|source| DataError::Polars {
            path: path.to_path_buf(),
            source,
        })?
        .finish()
        .map_err(|source| DataError::Polars {
            path: path.to_path_buf(),
            source,
        })?;
    let frame = normalize_csv_frame_types(raw_frame, path)?;

    let filename = file_name(path)?;
    let name = file_stem(path)?.to_ascii_uppercase();
    let variables = column_names(&frame)
        .into_iter()
        .map(|name| DatasetVariable {
            name,
            label: None,
            variable_type: None,
            length: None,
            extra: BTreeMap::new(),
        })
        .collect();

    let metadata = DatasetMetadata {
        name: name.clone(),
        domain: Some(name),
        label: None,
        filename,
        full_path: canonical_or_original(path),
        source_format: DatasetSourceFormat::Csv,
        variables,
    };

    Ok(LoadedDataset::new(metadata, frame))
}

#[derive(Debug)]
struct CsvRecords {
    headers: Vec<String>,
    records: Vec<Vec<String>>,
}

fn read_csv_records(path: &Path) -> Result<CsvRecords> {
    let source = fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(source.as_bytes());
    let headers = reader
        .headers()
        .map_err(|source| DataError::CsvParse {
            path: path.to_path_buf(),
            source,
        })?
        .iter()
        .map(|header| header.trim().to_owned())
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for record in reader.records() {
        records.push(
            record
                .map_err(|source| DataError::CsvParse {
                    path: path.to_path_buf(),
                    source,
                })?
                .iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
    }
    Ok(CsvRecords { headers, records })
}

fn read_csv_dict_rows(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    let records = read_csv_records(path)?;
    Ok(csv_records_to_dict_rows(&records))
}

fn csv_records_to_dict_rows(records: &CsvRecords) -> Vec<BTreeMap<String, String>> {
    records
        .records
        .iter()
        .map(|record| {
            records
                .headers
                .iter()
                .zip(record.iter())
                .map(|(key, value)| (key.clone(), value.trim().to_owned()))
                .collect::<BTreeMap<_, _>>()
        })
        .collect()
}

fn row_string(row: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            row.get(*key).or_else(|| {
                row.iter()
                    .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
                    .map(|(_key, value)| value)
            })
        })
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_dataset_name(value: &str) -> String {
    file_stem_str(value.trim()).to_ascii_uppercase()
}

fn normalize_metadata_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn normalize_csv_frame_types(frame: DataFrame, path: &Path) -> Result<DataFrame> {
    let height = frame.height();
    let mut columns = Vec::with_capacity(frame.width());
    for name in column_names(&frame) {
        let values = (0..height)
            .map(|row| cell_to_string(&frame, &name, row))
            .collect::<Result<Vec<_>>>()?;
        let inferred = infer_csv_column_values(&values);
        columns.push(series_from_json_values(&name, &inferred).into());
    }
    DataFrame::new(height, columns).map_err(|source| DataError::Polars {
        path: path.to_path_buf(),
        source,
    })
}

fn infer_csv_column_values(values: &[Option<String>]) -> Vec<Value> {
    if let Some(parsed) = parse_csv_column(values, parse_csv_bool) {
        return parsed
            .into_iter()
            .map(|value| value.map_or(Value::Null, Value::Bool))
            .collect();
    }
    if let Some(parsed) = parse_csv_column(values, parse_csv_i64) {
        return parsed
            .into_iter()
            .map(|value| {
                value.map_or(Value::Null, |value| {
                    Value::Number(serde_json::Number::from(value))
                })
            })
            .collect();
    }
    if let Some(parsed) = parse_csv_column(values, parse_csv_f64) {
        return parsed
            .into_iter()
            .map(|value| value.map_or(Value::Null, number_value))
            .collect();
    }

    values
        .iter()
        .map(|value| {
            value
                .as_ref()
                .map_or(Value::Null, |value| Value::String(value.clone()))
        })
        .collect()
}

fn parse_csv_column<T>(
    values: &[Option<String>],
    parser: impl Fn(&str) -> Option<T>,
) -> Option<Vec<Option<T>>> {
    let mut parsed = Vec::with_capacity(values.len());
    let mut saw_value = false;
    for value in values {
        let Some(value) = value else {
            parsed.push(None);
            continue;
        };
        let parsed_value = parser(value)?;
        saw_value = true;
        parsed.push(Some(parsed_value));
    }
    saw_value.then_some(parsed)
}

fn parse_csv_bool(value: &str) -> Option<bool> {
    if value != value.trim() {
        return None;
    }
    match value {
        "true" | "TRUE" | "True" => Some(true),
        "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

fn parse_csv_i64(value: &str) -> Option<i64> {
    if value != value.trim() || value.contains('.') || value.contains('e') || value.contains('E') {
        return None;
    }
    let digits = value.strip_prefix('-').unwrap_or(value);
    if !is_canonical_integer_digits(digits) {
        return None;
    }
    value.parse().ok()
}

fn parse_csv_f64(value: &str) -> Option<f64> {
    let has_float_marker = value.contains('.') || value.contains('e') || value.contains('E');
    if value != value.trim() || !has_float_marker {
        return None;
    }
    let exponent_index = value.find('e').or_else(|| value.find('E'));
    let mantissa = exponent_index.map_or(value, |index| &value[..index]);
    let unsigned_mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let integer_part = unsigned_mantissa
        .split_once('.')
        .map_or(unsigned_mantissa, |(integer, _fraction)| integer);
    if !is_canonical_integer_digits(integer_part) {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn is_canonical_integer_digits(value: &str) -> bool {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    value == "0" || !value.starts_with('0')
}

pub fn load_dataset_package_json(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let package: DatasetPackageJson =
        serde_json::from_str(&source).map_err(|source| DataError::JsonParse {
            path: path.to_path_buf(),
            source,
        })?;

    package
        .datasets
        .into_iter()
        .enumerate()
        .map(|(index, dataset)| dataset_package_entry_to_loaded_dataset(path, index, dataset))
        .collect()
}

pub fn load_xpt_dataset(path: impl AsRef<Path>) -> Result<LoadedDataset> {
    let path = path.as_ref();
    let metadata = fs::metadata(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > XPT_MAX_FILE_BYTES as u64 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT file exceeds maximum supported size of {XPT_MAX_FILE_BYTES} bytes"
        )));
    }
    let bytes = fs::read(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed = parse_xpt_v5(&bytes)?;
    let frame = records_to_frame(&parsed.records).map_err(|source| DataError::Polars {
        path: path.to_path_buf(),
        source,
    })?;

    let filename = file_name(path)?;
    let stem = file_stem(path)?.to_ascii_uppercase();
    let name = parsed.dataset_name.unwrap_or_else(|| stem.clone());
    let metadata = DatasetMetadata {
        name: name.clone(),
        domain: Some(name),
        label: parsed.dataset_label,
        filename,
        full_path: canonical_or_original(path),
        source_format: DatasetSourceFormat::Xpt,
        variables: parsed.variables,
    };

    Ok(LoadedDataset::new(metadata, frame))
}

fn dataset_package_entry_to_loaded_dataset(
    package_path: &Path,
    index: usize,
    dataset: DatasetPackageDataset,
) -> Result<LoadedDataset> {
    let frame = records_to_frame(&dataset.records).map_err(|source| DataError::Polars {
        path: package_path.to_path_buf(),
        source,
    })?;

    let filename = dataset.filename.clone().unwrap_or_else(|| {
        dataset
            .domain
            .as_deref()
            .map(|domain| format!("{}.json", domain.to_ascii_lowercase()))
            .unwrap_or_else(|| format!("dataset-{index}.json"))
    });
    let name = dataset
        .domain
        .clone()
        .unwrap_or_else(|| file_stem_str(&filename).to_ascii_uppercase());

    let metadata = DatasetMetadata {
        name,
        domain: dataset.domain,
        label: dataset.label,
        filename,
        full_path: canonical_or_original(package_path),
        source_format: DatasetSourceFormat::DatasetPackageJson,
        variables: dataset.variables,
    };

    Ok(LoadedDataset::new(metadata, frame))
}

#[derive(Debug, Clone)]
struct ParsedXpt {
    dataset_name: Option<String>,
    dataset_label: Option<String>,
    variables: Vec<DatasetVariable>,
    records: IndexMap<String, Vec<Value>>,
}

#[derive(Debug, Clone)]
struct XptVariable {
    name: String,
    label: Option<String>,
    variable_type: XptVariableType,
    length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XptVariableType {
    Numeric,
    Character,
}

const XPT_CARD_LEN: usize = 80;
const XPT_NAMESTR_LEN: usize = 140;
const XPT_MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const XPT_MAX_VARIABLES: usize = 10_000;
const XPT_MAX_OBSERVATION_BYTES: usize = 1024 * 1024;
const XPT_MAX_ROWS: usize = 5_000_000;
const XPT_MAX_CELLS: usize = 50_000_000;

fn parse_xpt_v5(bytes: &[u8]) -> Result<ParsedXpt> {
    if bytes.len() > XPT_MAX_FILE_BYTES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT file exceeds maximum supported size of {XPT_MAX_FILE_BYTES} bytes"
        )));
    }
    if bytes.len() < XPT_CARD_LEN {
        return Err(DataError::InvalidDatasetPackage(
            "XPT file is shorter than one 80-byte record".to_owned(),
        ));
    }

    let namestr_header =
        find_xpt_header(bytes, "HEADER RECORD*******NAMESTR").ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT NAMESTR header not found".to_owned())
        })?;
    let variable_count = parse_xpt_header_count(
        &bytes[namestr_header..namestr_header + XPT_CARD_LEN],
    )
    .ok_or_else(|| {
        DataError::InvalidDatasetPackage("XPT NAMESTR header is missing variable count".to_owned())
    })?;
    if variable_count == 0 {
        return Err(DataError::InvalidDatasetPackage(
            "XPT NAMESTR header declares zero variables".to_owned(),
        ));
    }
    if variable_count > XPT_MAX_VARIABLES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT NAMESTR header declares too many variables: {variable_count}"
        )));
    }

    let namestr_start = namestr_header
        .checked_add(XPT_CARD_LEN)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT NAMESTR start overflow".to_owned()))?;
    let namestr_len = variable_count.checked_mul(XPT_NAMESTR_LEN).ok_or_else(|| {
        DataError::InvalidDatasetPackage("XPT NAMESTR length overflow".to_owned())
    })?;
    let namestr_end = namestr_start
        .checked_add(namestr_len)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT NAMESTR end overflow".to_owned()))?;
    if bytes.len() < namestr_end {
        return Err(DataError::InvalidDatasetPackage(
            "XPT file ended before all NAMESTR records were available".to_owned(),
        ));
    }

    let variables = (0..variable_count)
        .map(|index| {
            let offset = namestr_start + index * XPT_NAMESTR_LEN;
            parse_xpt_namestr(&bytes[offset..][..XPT_NAMESTR_LEN])
        })
        .collect::<Result<Vec<_>>>()?;
    let observation_len = variables
        .iter()
        .map(|variable| variable.length)
        .try_fold(0usize, |acc, length| acc.checked_add(length))
        .ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT observation length overflow".to_owned())
        })?;
    if observation_len == 0 {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation length is zero".to_owned(),
        ));
    }
    if observation_len > XPT_MAX_OBSERVATION_BYTES {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT observation length exceeds maximum supported size of {XPT_MAX_OBSERVATION_BYTES} bytes"
        )));
    }

    let rounded_namestr_len = round_up_to_card(namestr_len)?;
    let mut data_start = namestr_start
        .checked_add(rounded_namestr_len)
        .ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT observation data start overflow".to_owned())
        })?;
    if bytes
        .get(data_start..data_start.saturating_add(XPT_CARD_LEN))
        .is_some_and(|card| ascii_card(card).starts_with("HEADER RECORD*******OBS"))
    {
        data_start = data_start.checked_add(XPT_CARD_LEN).ok_or_else(|| {
            DataError::InvalidDatasetPackage("XPT OBS header end overflow".to_owned())
        })?;
    }
    if data_start > bytes.len() {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation data starts beyond end of file".to_owned(),
        ));
    }

    let row_count = observation_row_count(&bytes[data_start..], observation_len);
    if row_count > XPT_MAX_ROWS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT row count exceeds maximum supported count of {XPT_MAX_ROWS}"
        )));
    }
    let cell_count = row_count
        .checked_mul(variable_count)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT cell count overflow".to_owned()))?;
    if cell_count > XPT_MAX_CELLS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT cell count exceeds maximum supported count of {XPT_MAX_CELLS}"
        )));
    }
    let mut records = variables
        .iter()
        .map(|variable| (variable.name.clone(), Vec::with_capacity(row_count)))
        .collect::<IndexMap<_, _>>();

    for row in observation_chunks(&bytes[data_start..], observation_len, row_count) {
        let mut offset = 0;
        for variable in &variables {
            let field = &row[offset..offset + variable.length];
            let value = match variable.variable_type {
                XptVariableType::Numeric => decode_xpt_numeric(field),
                XptVariableType::Character => {
                    Value::String(trim_xpt_text(field).unwrap_or_default())
                }
            };
            records
                .get_mut(&variable.name)
                .expect("record column initialized")
                .push(value);
            offset += variable.length;
        }
    }

    Ok(ParsedXpt {
        dataset_name: parse_xpt_dataset_name(bytes),
        dataset_label: None,
        variables: variables
            .into_iter()
            .map(|variable| DatasetVariable {
                name: variable.name,
                label: variable.label,
                variable_type: Some(match variable.variable_type {
                    XptVariableType::Numeric => "Num".to_owned(),
                    XptVariableType::Character => "Char".to_owned(),
                }),
                length: Some(variable.length),
                extra: BTreeMap::new(),
            })
            .collect(),
        records,
    })
}

fn find_xpt_header(bytes: &[u8], header: &str) -> Option<usize> {
    bytes
        .chunks_exact(XPT_CARD_LEN)
        .enumerate()
        .find(|(_index, card)| ascii_card(card).starts_with(header))
        .map(|(index, _card)| index * XPT_CARD_LEN)
}

fn parse_xpt_header_count(card: &[u8]) -> Option<usize> {
    ascii_card(card)
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<usize>().ok())
        .find(|value| *value > 0)
}

fn parse_xpt_namestr(bytes: &[u8]) -> Result<XptVariable> {
    if bytes.len() != XPT_NAMESTR_LEN {
        return Err(DataError::InvalidDatasetPackage(
            "XPT NAMESTR record has invalid length".to_owned(),
        ));
    }

    let ntype = read_xpt_u16(&bytes[0..2]);
    let length = read_xpt_u16(&bytes[4..6]) as usize;
    let name = trim_xpt_text(&bytes[8..16]).unwrap_or_default();
    if name.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "XPT variable has an empty name".to_owned(),
        ));
    }
    if length == 0 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT variable {name} has zero length"
        )));
    }

    let variable_type = match ntype {
        1 => XptVariableType::Numeric,
        2 => XptVariableType::Character,
        other => {
            return Err(DataError::InvalidDatasetPackage(format!(
                "XPT variable {name} has unsupported type {other}"
            )))
        }
    };
    if matches!(variable_type, XptVariableType::Numeric) && length > 8 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "XPT numeric variable {name} has unsupported length {length}"
        )));
    }

    Ok(XptVariable {
        name,
        label: trim_xpt_text(&bytes[16..56]).filter(|label| !label.is_empty()),
        variable_type,
        length,
    })
}

fn parse_xpt_dataset_name(bytes: &[u8]) -> Option<String> {
    bytes.chunks_exact(XPT_CARD_LEN).find_map(|card| {
        let card = ascii_card(card);
        let mut parts = card.split_whitespace();
        if parts.next()? == "SAS" {
            let candidate = parts.next()?.trim();
            if !candidate.eq_ignore_ascii_case("SAS") && !candidate.eq_ignore_ascii_case("SASLIB") {
                return Some(candidate.to_ascii_uppercase());
            }
        }
        None
    })
}

fn observation_row_count(data: &[u8], observation_len: usize) -> usize {
    let mut row_count = data.len() / observation_len;
    while row_count > 0 {
        let start = (row_count - 1) * observation_len;
        let row = &data[start..start + observation_len];
        if !row.iter().all(|byte| matches!(*byte, 0 | b' ')) {
            break;
        }
        row_count -= 1;
    }
    row_count
}

fn observation_chunks(
    data: &[u8],
    observation_len: usize,
    row_count: usize,
) -> impl Iterator<Item = &[u8]> {
    data.chunks_exact(observation_len).take(row_count)
}

fn decode_xpt_numeric(bytes: &[u8]) -> Value {
    if bytes.split_first().is_some_and(|(first, rest)| {
        matches!(*first, b'.' | b'_' | b'A'..=b'Z') && rest.iter().all(|byte| *byte == 0)
    }) {
        return Value::Null;
    }
    let value = ibm_float_to_f64(bytes);
    if !value.is_finite() {
        return Value::Null;
    }
    if (value.fract().abs() < f64::EPSILON) && value >= i64::MIN as f64 && value <= i64::MAX as f64
    {
        Value::Number(serde_json::Number::from(value as i64))
    } else {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn ibm_float_to_f64(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let sign = if bytes[0] & 0x80 == 0 { 1.0 } else { -1.0 };
    let exponent = (bytes[0] & 0x7f) as i32 - 64;
    let fraction = bytes
        .iter()
        .skip(1)
        .fold(0_u64, |acc, byte| (acc << 8) | u64::from(*byte));
    if fraction == 0 {
        return 0.0;
    }

    let fraction_bits = 8 * (bytes.len().saturating_sub(1) as i32);
    sign * (fraction as f64 / 2_f64.powi(fraction_bits)) * 16_f64.powi(exponent)
}

fn read_xpt_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn trim_xpt_text(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(*byte, 0 | b' '))
        .map(|index| index + 1)
        .unwrap_or(0);
    let start = bytes[..end]
        .iter()
        .position(|byte| !matches!(*byte, 0 | b' '))
        .unwrap_or(end);
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .map(str::to_owned)
}

fn ascii_card(card: &[u8]) -> String {
    String::from_utf8_lossy(card).into_owned()
}

fn round_up_to_card(value: usize) -> Result<usize> {
    value
        .div_ceil(XPT_CARD_LEN)
        .checked_mul(XPT_CARD_LEN)
        .ok_or_else(|| DataError::InvalidDatasetPackage("XPT card length overflow".to_owned()))
}

fn records_to_frame(records: &IndexMap<String, Vec<Value>>) -> PolarsResult<DataFrame> {
    if records.is_empty() {
        return Ok(DataFrame::empty());
    }

    let expected_len = records.values().next().map(Vec::len).unwrap_or_default();
    let mut columns = Vec::with_capacity(records.len());

    for (name, values) in records {
        if values.len() != expected_len {
            polars_bail!(
                ComputeError:
                "record column '{}' has length {}, expected {}",
                name,
                values.len(),
                expected_len
            );
        }
        columns.push(series_from_json_values(name, values).into());
    }

    DataFrame::new(expected_len, columns)
}

fn series_from_json_values(name: &str, values: &[Value]) -> Series {
    if values
        .iter()
        .all(|value| value.is_null() || value.as_bool().is_some())
    {
        let typed: Vec<Option<bool>> = values.iter().map(Value::as_bool).collect();
        return Series::new(name.into(), typed);
    }

    if values
        .iter()
        .all(|value| value.is_null() || value.as_i64().is_some())
    {
        let typed: Vec<Option<i64>> = values.iter().map(Value::as_i64).collect();
        return Series::new(name.into(), typed);
    }

    if values
        .iter()
        .all(|value| value.is_null() || value.as_f64().is_some())
    {
        let typed: Vec<Option<f64>> = values.iter().map(Value::as_f64).collect();
        return Series::new(name.into(), typed);
    }

    let typed: Vec<Option<String>> = values
        .iter()
        .map(|value| match value {
            Value::Null => None,
            Value::String(value) => Some(value.clone()),
            other => Some(other.to_string()),
        })
        .collect();
    Series::new(name.into(), typed)
}

fn column_names(frame: &DataFrame) -> Vec<String> {
    frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str().to_owned())
        .collect()
}

#[derive(Debug, Deserialize)]
struct DatasetPackageJson {
    datasets: Vec<DatasetPackageDataset>,
}

#[derive(Debug, Deserialize)]
struct DatasetPackageDataset {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    variables: Vec<DatasetVariable>,
    records: IndexMap<String, Vec<Value>>,
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| DataError::InvalidDatasetPackage(format!("missing file name: {path:?}")))
}

fn file_stem(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| DataError::InvalidDatasetPackage(format!("missing file stem: {path:?}")))
}

fn file_stem_str(filename: &str) -> &str {
    Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
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

pub fn filter_dataset_by_mask(dataset: &LoadedDataset, mask: &[bool]) -> Result<LoadedDataset> {
    if mask.len() != dataset.frame.height() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "filter mask length {} does not match row count {}",
            mask.len(),
            dataset.frame.height()
        )));
    }

    let indices = mask
        .iter()
        .enumerate()
        .filter_map(|(index, keep)| keep.then_some(index as u32))
        .collect::<Vec<_>>();
    take_dataset_rows(dataset, &indices)
}

pub fn derive_literal_column(
    dataset: &LoadedDataset,
    column_name: &str,
    value: &Value,
) -> Result<LoadedDataset> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "derive operation requires a target column".to_owned(),
        ));
    }
    if dataset.frame.column(column_name).is_ok() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "derived column already exists: {column_name}"
        )));
    }

    let values = (0..dataset.frame.height())
        .map(|_| value.clone())
        .collect::<Vec<_>>();
    let mut frame = dataset.frame.clone();
    frame
        .hstack_mut(&[series_from_json_values(column_name, &values).into()])
        .map_err(|source| DataError::Polars {
            path: dataset.metadata.full_path.clone(),
            source,
        })?;

    Ok(LoadedDataset::new(dataset.metadata.clone(), frame))
}

pub fn derive_column_from_column(
    dataset: &LoadedDataset,
    column_name: &str,
    source_column: &str,
) -> Result<LoadedDataset> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "derive operation requires a target column".to_owned(),
        ));
    }
    if dataset.frame.column(column_name).is_ok() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "derived column already exists: {column_name}"
        )));
    }
    if dataset.frame.column(source_column).is_err() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "derive source column not found: {source_column}"
        )));
    }

    let values = (0..dataset.frame.height())
        .map(|row| cell_to_json_value(&dataset.frame, source_column, row))
        .collect::<Result<Vec<_>>>()?;
    derive_column_from_values(dataset, column_name, &values)
}

pub fn derive_column_from_values(
    dataset: &LoadedDataset,
    column_name: &str,
    values: &[Value],
) -> Result<LoadedDataset> {
    if values.len() != dataset.frame.height() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "derived column length {} does not match row count {}",
            values.len(),
            dataset.frame.height()
        )));
    }
    derive_literal_series(dataset, column_name, values)
}

pub fn dataset_column_values(dataset: &LoadedDataset, column_name: &str) -> Result<Vec<Value>> {
    if dataset.frame.column(column_name).is_err() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "column not found: {column_name}"
        )));
    }
    (0..dataset.frame.height())
        .map(|row| cell_to_json_value(&dataset.frame, column_name, row))
        .collect()
}

pub fn group_count_dataset(
    dataset: &LoadedDataset,
    keys: &[String],
    column_name: &str,
) -> Result<LoadedDataset> {
    if keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "group count operation requires at least one key".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "group count operation requires an output column".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "group count key not found: {key}"
            )));
        }
    }

    let mut counts = HashMap::new();
    for row in 0..dataset.frame.height() {
        *counts
            .entry(row_key(&dataset.frame, keys, row)?)
            .or_insert(0_i64) += 1;
    }

    let values = (0..dataset.frame.height())
        .map(|row| {
            row_key(&dataset.frame, keys, row).map(|key| {
                Value::Number(serde_json::Number::from(
                    *counts.get(&key).unwrap_or(&0_i64),
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    derive_literal_series(dataset, column_name, &values)
}

pub fn group_stat_dataset(
    dataset: &LoadedDataset,
    keys: &[String],
    source_column: Option<&str>,
    column_name: &str,
    statistic: &str,
) -> Result<LoadedDataset> {
    if keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "aggregate operation requires at least one key".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "aggregate operation requires an output column".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "aggregate key not found: {key}"
            )));
        }
    }

    let statistic = normalize_statistic_name(statistic);
    let needs_source = matches!(
        statistic.as_str(),
        "sum" | "mean" | "avg" | "average" | "min" | "max" | "count_distinct" | "distinct_count"
    );
    if needs_source && source_column.is_none() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "aggregate {statistic} requires a source column"
        )));
    }
    if let Some(source_column) = source_column {
        if dataset.frame.column(source_column).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "aggregate source column not found: {source_column}"
            )));
        }
    }

    let mut groups: HashMap<Vec<RowKeyValue>, GroupAccumulator> = HashMap::new();
    for row in 0..dataset.frame.height() {
        let key = row_key(&dataset.frame, keys, row)?;
        let accumulator = groups.entry(key).or_default();
        accumulator.count += 1;

        if let Some(source_column) = source_column {
            if let Some(value) = cell_to_string(&dataset.frame, source_column, row)? {
                accumulator.distinct.insert(value.clone());
                if let Ok(number) = value.parse::<f64>() {
                    accumulator.numeric_count += 1;
                    accumulator.sum += number;
                    accumulator.min = Some(
                        accumulator
                            .min
                            .map(|existing| existing.min(number))
                            .unwrap_or(number),
                    );
                    accumulator.max = Some(
                        accumulator
                            .max
                            .map(|existing| existing.max(number))
                            .unwrap_or(number),
                    );
                }
            }
        }
    }

    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = row_key(&dataset.frame, keys, row)?;
            let accumulator = groups.get(&key).ok_or_else(|| {
                DataError::InvalidDatasetPackage("aggregate group was not found".to_owned())
            })?;
            Ok(aggregate_value(accumulator, &statistic))
        })
        .collect::<Result<Vec<_>>>()?;
    derive_column_from_values(dataset, column_name, &values)
}

pub fn group_distinct_values_dataset(
    dataset: &LoadedDataset,
    keys: &[String],
    source_column: &str,
    column_name: &str,
) -> Result<LoadedDataset> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires an output column".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "distinct values key not found: {key}"
            )));
        }
    }
    if dataset.frame.column(source_column).is_err() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "distinct values source column not found: {source_column}"
        )));
    }

    let mut groups: HashMap<Vec<RowKeyValue>, BTreeSet<String>> = HashMap::new();
    for row in 0..dataset.frame.height() {
        if let Some(value) = cell_to_string(&dataset.frame, source_column, row)? {
            groups
                .entry(row_key(&dataset.frame, keys, row)?)
                .or_default()
                .insert(value);
        }
    }

    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = row_key(&dataset.frame, keys, row)?;
            let joined = groups
                .get(&key)
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default();
            Ok(Value::String(joined))
        })
        .collect::<Result<Vec<_>>>()?;
    derive_column_from_values(dataset, column_name, &values)
}

pub fn row_number_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
    keys: &[String],
) -> Result<LoadedDataset> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "row number operation requires an output column".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "row number key not found: {key}"
            )));
        }
    }

    let mut counters: HashMap<Vec<RowKeyValue>, i64> = HashMap::new();
    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = if keys.is_empty() {
                vec![RowKeyValue::String("<ALL>".to_owned())]
            } else {
                row_key(&dataset.frame, keys, row)?
            };
            let counter = counters.entry(key).or_insert(0);
            *counter += 1;
            Ok(Value::Number(serde_json::Number::from(*counter)))
        })
        .collect::<Result<Vec<_>>>()?;
    derive_column_from_values(dataset, column_name, &values)
}

pub fn select_dataset_columns(
    dataset: &LoadedDataset,
    columns: &[String],
) -> Result<LoadedDataset> {
    if columns.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "select operation requires at least one column".to_owned(),
        ));
    }

    let selected = columns
        .iter()
        .map(|column| {
            dataset
                .frame
                .column(column)
                .cloned()
                .map_err(|source| DataError::Polars {
                    path: dataset.metadata.full_path.clone(),
                    source,
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let frame =
        DataFrame::new(dataset.frame.height(), selected).map_err(|source| DataError::Polars {
            path: dataset.metadata.full_path.clone(),
            source,
        })?;
    Ok(LoadedDataset::new(dataset.metadata.clone(), frame))
}

pub fn drop_dataset_columns(dataset: &LoadedDataset, columns: &[String]) -> Result<LoadedDataset> {
    let drop = columns
        .iter()
        .map(|column| column.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let keep = dataset
        .frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str().to_owned())
        .filter(|name| !drop.contains(&name.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    if keep.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "drop operation cannot remove all columns".to_owned(),
        ));
    }
    select_dataset_columns(dataset, &keep)
}

pub fn rename_dataset_columns(
    dataset: &LoadedDataset,
    renames: &BTreeMap<String, String>,
) -> Result<LoadedDataset> {
    if renames.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "rename operation requires at least one column mapping".to_owned(),
        ));
    }

    let mut columns = Vec::with_capacity(dataset.frame.width());
    for column in dataset.frame.get_column_names() {
        let original = column.as_str();
        let mut renamed =
            dataset
                .frame
                .column(original)
                .cloned()
                .map_err(|source| DataError::Polars {
                    path: dataset.metadata.full_path.clone(),
                    source,
                })?;
        if let Some(new_name) = renames
            .iter()
            .find(|(from, _to)| from.eq_ignore_ascii_case(original))
            .map(|(_from, to)| to)
        {
            renamed.rename(new_name.into());
        }
        columns.push(renamed);
    }

    let frame =
        DataFrame::new(dataset.frame.height(), columns).map_err(|source| DataError::Polars {
            path: dataset.metadata.full_path.clone(),
            source,
        })?;
    Ok(LoadedDataset::new(dataset.metadata.clone(), frame))
}

pub fn deduplicate_dataset_by_columns(
    dataset: &LoadedDataset,
    keys: &[String],
) -> Result<LoadedDataset> {
    let keys = if keys.is_empty() {
        column_names(&dataset.frame)
    } else {
        keys.to_vec()
    };
    for key in &keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "distinct key not found: {key}"
            )));
        }
    }

    let mut seen = HashSet::new();
    let indices = (0..dataset.frame.height())
        .filter_map(|row| {
            row_key(&dataset.frame, &keys, row)
                .map(|key| seen.insert(key).then_some(row as u32))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    take_dataset_rows(dataset, &indices)
}

fn derive_literal_series(
    dataset: &LoadedDataset,
    column_name: &str,
    values: &[Value],
) -> Result<LoadedDataset> {
    if dataset.frame.column(column_name).is_ok() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "derived column already exists: {column_name}"
        )));
    }

    let mut frame = dataset.frame.clone();
    frame
        .hstack_mut(&[series_from_json_values(column_name, values).into()])
        .map_err(|source| DataError::Polars {
            path: dataset.metadata.full_path.clone(),
            source,
        })?;
    Ok(LoadedDataset::new(dataset.metadata.clone(), frame))
}

#[derive(Debug, Default)]
struct GroupAccumulator {
    count: i64,
    numeric_count: i64,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
    distinct: BTreeSet<String>,
}

fn aggregate_value(accumulator: &GroupAccumulator, statistic: &str) -> Value {
    match statistic {
        "count_distinct" | "distinct_count" => {
            Value::Number(serde_json::Number::from(accumulator.distinct.len() as i64))
        }
        "sum" => number_value(accumulator.sum),
        "mean" | "avg" | "average" => {
            if accumulator.numeric_count == 0 {
                Value::Null
            } else {
                number_value(accumulator.sum / accumulator.numeric_count as f64)
            }
        }
        "min" => accumulator.min.map(number_value).unwrap_or(Value::Null),
        "max" => accumulator.max.map(number_value).unwrap_or(Value::Null),
        _ => Value::Number(serde_json::Number::from(accumulator.count)),
    }
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

fn normalize_statistic_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
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

pub fn left_join_dataset(
    left: &LoadedDataset,
    right: &LoadedDataset,
    keys: &[String],
    right_prefix: &str,
) -> Result<LoadedDataset> {
    left_join_dataset_on(left, right, keys, keys, right_prefix)
}

pub fn left_join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
    right_prefix: &str,
) -> Result<LoadedDataset> {
    join_dataset_on(left, right, left_keys, right_keys, right_prefix, true)
}

pub fn inner_join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
    right_prefix: &str,
) -> Result<LoadedDataset> {
    join_dataset_on(left, right, left_keys, right_keys, right_prefix, false)
}

fn join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
    right_prefix: &str,
    include_unmatched_left: bool,
) -> Result<LoadedDataset> {
    let (left_keys, right_keys) = resolve_join_key_pair(left, right, left_keys, right_keys)?;

    let mut index: HashMap<Vec<RowKeyValue>, Vec<usize>> = HashMap::new();
    for row in 0..right.frame.height() {
        index
            .entry(row_key(&right.frame, &right_keys, row)?)
            .or_default()
            .push(row);
    }

    let mut left_rows = Vec::new();
    let mut right_rows = Vec::new();
    for left_row in 0..left.frame.height() {
        let key = row_key(&left.frame, &left_keys, left_row)?;
        if let Some(matches) = index.get(&key) {
            for right_row in matches {
                left_rows.push(left_row as u32);
                right_rows.push(Some(*right_row));
            }
        } else if include_unmatched_left {
            left_rows.push(left_row as u32);
            right_rows.push(None);
        }
    }

    let left_indices = UInt32Chunked::from_vec("row_index".into(), left_rows);
    let mut frame = left
        .frame
        .take(&left_indices)
        .map_err(|source| DataError::Polars {
            path: left.metadata.full_path.clone(),
            source,
        })?;
    let left_columns = left
        .frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str().to_owned())
        .collect::<BTreeSet<_>>();

    let mut joined_columns = Vec::new();
    for right_column in right.frame.get_column_names() {
        let right_column = right_column.as_str();
        if !right_prefix.is_empty() && right_keys.iter().any(|key| key == right_column) {
            continue;
        }

        let joined_name = format!("{right_prefix}{right_column}");
        if left_columns.contains(&joined_name) {
            continue;
        }

        let values = right_rows
            .iter()
            .map(|right_row| {
                right_row.map_or(Ok(Value::Null), |row| {
                    cell_to_json_value(&right.frame, right_column, row)
                })
            })
            .collect::<Result<Vec<_>>>()?;
        joined_columns.push(series_from_json_values(&joined_name, &values).into());
    }

    frame
        .hstack_mut(&joined_columns)
        .map_err(|source| DataError::Polars {
            path: left.metadata.full_path.clone(),
            source,
        })?;

    Ok(LoadedDataset::new(left.metadata.clone(), frame))
}

pub fn semi_join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
) -> Result<LoadedDataset> {
    filter_join_matches(left, right, left_keys, right_keys, true)
}

pub fn anti_join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
) -> Result<LoadedDataset> {
    filter_join_matches(left, right, left_keys, right_keys, false)
}

fn filter_join_matches(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
    keep_matches: bool,
) -> Result<LoadedDataset> {
    let (left_keys, right_keys) = resolve_join_key_pair(left, right, left_keys, right_keys)?;
    let mut index = HashSet::new();
    for row in 0..right.frame.height() {
        index.insert(row_key(&right.frame, &right_keys, row)?);
    }

    let indices = (0..left.frame.height())
        .filter_map(|row| {
            row_key(&left.frame, &left_keys, row)
                .map(|key| (index.contains(&key) == keep_matches).then_some(row as u32))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    take_dataset_rows(left, &indices)
}

fn resolve_join_key_pair(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    if left_keys.is_empty() || right_keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "join requires at least one key".to_owned(),
        ));
    }
    if left_keys.len() != right_keys.len() {
        return Err(DataError::InvalidDatasetPackage(
            "left and right join keys must have the same length".to_owned(),
        ));
    }

    Ok((
        resolve_join_keys(left, left_keys, "left")?,
        resolve_join_keys(right, right_keys, "right")?,
    ))
}

fn resolve_join_keys(dataset: &LoadedDataset, keys: &[String], side: &str) -> Result<Vec<String>> {
    keys.iter()
        .map(|key| {
            actual_column_name(&dataset.frame, key).ok_or_else(|| {
                DataError::InvalidDatasetPackage(format!("{side} join key not found: {key}"))
            })
        })
        .collect()
}

fn actual_column_name(frame: &DataFrame, name: &str) -> Option<String> {
    if frame.column(name).is_ok() {
        return Some(name.to_owned());
    }
    frame
        .get_column_names()
        .into_iter()
        .find(|column| column.as_str().eq_ignore_ascii_case(name))
        .map(|column| column.as_str().to_owned())
}

pub(crate) fn row_key(frame: &DataFrame, keys: &[String], row: usize) -> Result<Vec<RowKeyValue>> {
    keys.iter()
        .map(|key| cell_to_key(frame, key, row))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RowKeyValue {
    Null,
    Bool(bool),
    Number(NumberKey),
    String(String),
}

impl RowKeyValue {
    fn from_any_value(value: AnyValue<'_>) -> Self {
        if value.is_null() {
            return Self::Null;
        }
        if let Some(value) = value.extract_bool() {
            return Self::Bool(value);
        }
        if let Some(value) = value.extract_str() {
            return Self::String(value.to_owned());
        }
        if let Some(value) = value.extract::<f64>() {
            return Self::Number(NumberKey::new(value));
        }
        Self::String(value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NumberKey(u64);

impl NumberKey {
    fn new(value: f64) -> Self {
        let value = if value == 0.0 { 0.0 } else { value };
        Self(value.to_bits())
    }

    fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl PartialOrd for NumberKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NumberKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value().total_cmp(&other.value())
    }
}

fn cell_to_key(frame: &DataFrame, column_name: &str, row: usize) -> Result<RowKeyValue> {
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
    Ok(RowKeyValue::from_any_value(value))
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
