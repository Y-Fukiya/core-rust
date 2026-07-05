#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
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
mod usdm_common;
mod usdm_content;
mod usdm_data_dir;
mod usdm_data_dir_datasets;
mod usdm_design;
mod usdm_geography;
mod usdm_identifiers;
mod usdm_json_schema;
mod usdm_objects;
mod usdm_population_columns;
mod usdm_product;
mod usdm_references;
mod usdm_row_builders;
mod usdm_study_structure;
mod usdm_text_templates;
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
use json_table::series_from_json_values;
pub use json_table::{metadata_row_dataset, metadata_rows_dataset};
pub use open_rules_data_dir::{load_open_rules_data_dir, load_open_rules_data_dir_with_warnings};
pub(crate) use row_key::{row_key, RowKeyValue};
pub(crate) use usdm_data_dir::load_open_rules_json_data_dir;
pub use xpt::load_xpt_dataset;
#[cfg(test)]
pub(crate) use xpt::{XptVariableType, XPT_CARD_LEN, XPT_MAX_FILE_BYTES, XPT_NAMESTR_LEN};

pub type Result<T> = std::result::Result<T, DataError>;

pub(crate) const DATASET_MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
pub(crate) const DATASET_MAX_ROWS: usize = 5_000_000;
pub(crate) const DATASET_MAX_CELLS: usize = 50_000_000;

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

pub(crate) fn validate_dataset_file_size(path: &Path, format: &str) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|source| DataError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > DATASET_MAX_FILE_BYTES as u64 {
        return Err(DataError::InvalidDatasetPackage(format!(
            "{format} file exceeds maximum supported size of {DATASET_MAX_FILE_BYTES} bytes"
        )));
    }
    Ok(())
}

pub(crate) fn validate_frame_limits(frame: &DataFrame, format: &str) -> Result<()> {
    let row_count = frame.height();
    if row_count > DATASET_MAX_ROWS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "{format} row count exceeds maximum supported count of {DATASET_MAX_ROWS}"
        )));
    }
    let cell_count = row_count
        .checked_mul(frame.width())
        .ok_or_else(|| DataError::InvalidDatasetPackage(format!("{format} cell count overflow")))?;
    if cell_count > DATASET_MAX_CELLS {
        return Err(DataError::InvalidDatasetPackage(format!(
            "{format} cell count exceeds maximum supported count of {DATASET_MAX_CELLS}"
        )));
    }
    Ok(())
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
