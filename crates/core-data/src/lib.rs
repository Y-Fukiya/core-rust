#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use polars::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

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

pub fn load_csv_dataset(path: impl AsRef<Path>) -> Result<LoadedDataset> {
    let path = path.as_ref();
    let frame = CsvReadOptions::default()
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

fn records_to_frame(records: &BTreeMap<String, Vec<Value>>) -> PolarsResult<DataFrame> {
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
    records: BTreeMap<String, Vec<Value>>,
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
    matches!(extension(path).as_deref(), Some("csv" | "json"))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn load_csv_dataset_builds_metadata_and_summary() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("AE.csv");
        fs::write(
            &path,
            "STUDYID,DOMAIN,AESEQ\nCDISC-TEST,AE,1\nCDISC-TEST,AE,2\n",
        )
        .expect("write csv");

        let dataset = load_csv_dataset(&path).expect("load csv");
        let summary = dataset.summary();

        assert_eq!(dataset.metadata().name, "AE");
        assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
        assert_eq!(summary.filename, "AE.csv");
        assert_eq!(summary.row_count, 2);
        assert_eq!(summary.columns, vec!["STUDYID", "DOMAIN", "AESEQ"]);
        assert_eq!(dataset.frame().height(), 2);
    }

    #[test]
    fn load_dataset_package_json_builds_datasets() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "label": "Adverse Events",
      "domain": "AE",
      "variables": [
        {
          "name": "STUDYID",
          "label": "Study Identifier",
          "type": "Char",
          "length": 10
        },
        {
          "name": "AESEQ",
          "label": "Sequence Number",
          "type": "Num",
          "length": 8
        }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST", "CDISC-TEST"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let datasets = load_dataset_package_json(&path).expect("load package");
        let dataset = &datasets[0];
        let summary = dataset.summary();

        assert_eq!(datasets.len(), 1);
        assert_eq!(dataset.metadata().name, "AE");
        assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
        assert_eq!(dataset.metadata().label.as_deref(), Some("Adverse Events"));
        assert_eq!(dataset.metadata().filename, "ae.xpt");
        assert_eq!(dataset.metadata().variables.len(), 2);
        assert_eq!(summary.row_count, 2);
        assert_eq!(summary.columns, vec!["AESEQ", "DOMAIN", "STUDYID"]);
    }

    #[test]
    fn load_datasets_from_directory_scans_direct_children_only() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("AE.csv"), "STUDYID,DOMAIN\nS1,AE\n").expect("write csv");
        fs::write(
            dir.path().join("package.json"),
            r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["CM"]
      }
    }
  ]
}"#,
        )
        .expect("write package");
        fs::write(dir.path().join("notes.txt"), "ignore me").expect("write notes");

        let nested = dir.path().join("nested");
        fs::create_dir(&nested).expect("create nested");
        fs::write(nested.join("VS.csv"), "STUDYID,DOMAIN\nS1,VS\n").expect("write nested csv");

        let result = load_datasets_from_paths_with_warnings(&[dir.path().to_path_buf()])
            .expect("load directory");

        assert_eq!(result.datasets.len(), 2);
        assert_eq!(
            dataset_names(&result.datasets),
            BTreeSet::from(["AE".to_owned(), "CM".to_owned()])
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].kind,
            LoadDataWarningKind::UnsupportedExtension("txt".to_owned())
        );
    }

    #[test]
    fn package_json_rejects_mismatched_record_lengths() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("bad.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S2"],
        "DOMAIN": ["AE"]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let error = load_dataset_package_json(&path).expect_err("mismatched lengths fail");

        assert!(matches!(error, DataError::Polars { .. }));
    }
}
