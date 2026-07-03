use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::{file_stem_str, DataError, Result};

#[derive(Debug)]
pub(crate) struct CsvRecords {
    pub(crate) headers: Vec<String>,
    pub(crate) records: Vec<Vec<String>>,
}

pub(crate) fn read_csv_records(path: &Path) -> Result<CsvRecords> {
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

pub(crate) fn read_csv_dict_rows(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    let records = read_csv_records(path)?;
    Ok(csv_records_to_dict_rows(&records))
}

pub(crate) fn csv_records_to_dict_rows(records: &CsvRecords) -> Vec<BTreeMap<String, String>> {
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

pub(crate) fn row_string(row: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
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

pub(crate) fn normalize_dataset_name(value: &str) -> String {
    file_stem_str(value.trim()).to_ascii_uppercase()
}

pub(crate) fn normalize_metadata_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase()
}
