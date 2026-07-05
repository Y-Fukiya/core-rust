use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::json_table::records_to_frame;
use crate::{
    canonical_or_original, file_stem_str, validate_dataset_file_size, validate_frame_limits,
    DataError, DatasetMetadata, DatasetSourceFormat, DatasetVariable, LoadedDataset, Result,
};

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

pub fn load_dataset_package_json(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>> {
    let path = path.as_ref();
    validate_dataset_file_size(path, "DatasetPackage JSON")?;
    let source = std::fs::read_to_string(path).map_err(|source| DataError::Io {
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
    validate_frame_limits(&frame, "DatasetPackage JSON")?;

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
