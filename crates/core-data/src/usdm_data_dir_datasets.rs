use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::csv_records::normalize_dataset_name;
use crate::json_table::json_rows_dataset;
use crate::{LoadedDataset, Result};

pub(crate) fn push_usdm_dataset(
    data_dir: &Path,
    datasets: &mut Vec<LoadedDataset>,
    name: &str,
    file_name: &str,
    rows: &[BTreeMap<String, Value>],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    datasets.push(json_rows_dataset(data_dir, name, file_name, rows)?);
    Ok(())
}

pub(crate) fn push_usdm_dataset_even_when_empty(
    data_dir: &Path,
    datasets: &mut Vec<LoadedDataset>,
    name: &str,
    file_name: &str,
    rows: &[BTreeMap<String, Value>],
) -> Result<()> {
    datasets.push(json_rows_dataset(data_dir, name, file_name, rows)?);
    Ok(())
}

pub(crate) fn push_usdm_identifier_datasets(
    data_dir: &Path,
    datasets: &mut Vec<LoadedDataset>,
    identifier_rows: &[BTreeMap<String, Value>],
) -> Result<()> {
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
        push_usdm_dataset(
            data_dir,
            datasets,
            entity,
            &format!(
                "usdm-{}.json",
                normalize_dataset_name(entity).to_ascii_lowercase()
            ),
            &rows,
        )?;
    }
    Ok(())
}
