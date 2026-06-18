#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

pub fn load_xpt_dataset(path: impl AsRef<Path>) -> Result<LoadedDataset> {
    let path = path.as_ref();
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
    records: BTreeMap<String, Vec<Value>>,
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

fn parse_xpt_v5(bytes: &[u8]) -> Result<ParsedXpt> {
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

    let namestr_start = namestr_header + XPT_CARD_LEN;
    let namestr_len = variable_count * XPT_NAMESTR_LEN;
    if bytes.len() < namestr_start + namestr_len {
        return Err(DataError::InvalidDatasetPackage(
            "XPT file ended before all NAMESTR records were available".to_owned(),
        ));
    }

    let variables = (0..variable_count)
        .map(|index| {
            parse_xpt_namestr(&bytes[namestr_start + index * XPT_NAMESTR_LEN..][..XPT_NAMESTR_LEN])
        })
        .collect::<Result<Vec<_>>>()?;
    let observation_len = variables
        .iter()
        .map(|variable| variable.length)
        .sum::<usize>();
    if observation_len == 0 {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation length is zero".to_owned(),
        ));
    }

    let mut data_start = namestr_start + round_up_to_card(namestr_len);
    if bytes
        .get(data_start..data_start + XPT_CARD_LEN)
        .is_some_and(|card| ascii_card(card).starts_with("HEADER RECORD*******OBS"))
    {
        data_start += XPT_CARD_LEN;
    }
    if data_start > bytes.len() {
        return Err(DataError::InvalidDatasetPackage(
            "XPT observation data starts beyond end of file".to_owned(),
        ));
    }

    let row_chunks = observation_chunks(&bytes[data_start..], observation_len);
    let mut records = variables
        .iter()
        .map(|variable| (variable.name.clone(), Vec::with_capacity(row_chunks.len())))
        .collect::<BTreeMap<_, _>>();

    for row in row_chunks {
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

fn observation_chunks(data: &[u8], observation_len: usize) -> Vec<&[u8]> {
    let mut rows = data.chunks_exact(observation_len).collect::<Vec<_>>();
    while rows
        .last()
        .is_some_and(|row| row.iter().all(|byte| matches!(*byte, 0 | b' ')))
    {
        rows.pop();
    }
    rows
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

fn round_up_to_card(value: usize) -> usize {
    value.div_ceil(XPT_CARD_LEN) * XPT_CARD_LEN
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
        .map(|row| {
            cell_to_string(&dataset.frame, source_column, row)
                .map(|value| value.map(Value::String).unwrap_or(Value::Null))
        })
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
        .map(|row| {
            cell_to_string(&dataset.frame, column_name, row)
                .map(|value| value.map(Value::String).unwrap_or(Value::Null))
        })
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

    let mut groups: HashMap<Vec<String>, GroupAccumulator> = HashMap::new();
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

pub fn sort_dataset_by_columns(
    dataset: &LoadedDataset,
    keys: &[String],
    descending: bool,
) -> Result<LoadedDataset> {
    if keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "sort operation requires at least one key".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "sort key not found: {key}"
            )));
        }
    }

    let mut keyed_rows = (0..dataset.frame.height())
        .map(|row| row_key(&dataset.frame, keys, row).map(|key| (key, row as u32)))
        .collect::<Result<Vec<_>>>()?;
    keyed_rows.sort_by(|left, right| left.0.cmp(&right.0));
    if descending {
        keyed_rows.reverse();
    }
    let indices = keyed_rows
        .into_iter()
        .map(|(_key, row)| row)
        .collect::<Vec<_>>();
    take_dataset_rows(dataset, &indices)
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

    let mut counters: HashMap<Vec<String>, i64> = HashMap::new();
    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = if keys.is_empty() {
                vec!["<ALL>".to_owned()]
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

fn take_dataset_rows(dataset: &LoadedDataset, indices: &[u32]) -> Result<LoadedDataset> {
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
    validate_join_keys(left, right, left_keys, right_keys)?;

    let mut index = HashMap::new();
    for row in 0..right.frame.height() {
        index
            .entry(row_key(&right.frame, right_keys, row)?)
            .or_insert(row);
    }

    let mut frame = left.frame.clone();
    let left_columns = left
        .frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str().to_owned())
        .collect::<BTreeSet<_>>();

    let mut joined_columns = Vec::new();
    for right_column in right.frame.get_column_names() {
        let right_column = right_column.as_str();
        if right_keys.iter().any(|key| key == right_column) {
            continue;
        }

        let joined_name = format!("{right_prefix}{right_column}");
        if left_columns.contains(&joined_name) {
            continue;
        }

        let mut values = Vec::with_capacity(left.frame.height());
        for left_row in 0..left.frame.height() {
            let key = row_key(&left.frame, left_keys, left_row)?;
            let value = if let Some(right_row) = index.get(&key) {
                cell_to_string(&right.frame, right_column, *right_row)?
            } else {
                None
            };
            values.push(value);
        }
        joined_columns.push(Series::new(joined_name.into(), values).into());
    }

    frame
        .hstack_mut(&joined_columns)
        .map_err(|source| DataError::Polars {
            path: left.metadata.full_path.clone(),
            source,
        })?;

    Ok(LoadedDataset::new(left.metadata.clone(), frame))
}

pub fn inner_join_dataset_on(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
    right_prefix: &str,
) -> Result<LoadedDataset> {
    let matched = filter_join_matches(left, right, left_keys, right_keys, true)?;
    left_join_dataset_on(&matched, right, left_keys, right_keys, right_prefix)
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
    validate_join_keys(left, right, left_keys, right_keys)?;
    let mut index = HashSet::new();
    for row in 0..right.frame.height() {
        index.insert(row_key(&right.frame, right_keys, row)?);
    }

    let indices = (0..left.frame.height())
        .filter_map(|row| {
            row_key(&left.frame, left_keys, row)
                .map(|key| (index.contains(&key) == keep_matches).then_some(row as u32))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    take_dataset_rows(left, &indices)
}

fn validate_join_keys(
    left: &LoadedDataset,
    right: &LoadedDataset,
    left_keys: &[String],
    right_keys: &[String],
) -> Result<()> {
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

    for key in left_keys {
        if left.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "left join key not found: {key}"
            )));
        }
    }
    for key in right_keys {
        if right.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "right join key not found: {key}"
            )));
        }
    }
    Ok(())
}

fn row_key(frame: &DataFrame, keys: &[String], row: usize) -> Result<Vec<String>> {
    keys.iter()
        .map(|key| {
            cell_to_string(frame, key, row)
                .map(|value| value.unwrap_or_else(|| "<NULL>".to_owned()))
        })
        .collect()
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
    fn load_xpt_dataset_builds_metadata_and_rows() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ae.xpt");
        write_test_xpt(
            &path,
            "AE",
            &[
                TestXptVariable::character("STUDYID", 12, "Study Identifier"),
                TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
                TestXptVariable::numeric("AESEQ", "Sequence Number"),
            ],
            &[
                vec![
                    TestXptValue::Text("CDISC-TEST"),
                    TestXptValue::Text("AE"),
                    TestXptValue::Number(1.0),
                ],
                vec![
                    TestXptValue::Text("CDISC-TEST"),
                    TestXptValue::Text("AE"),
                    TestXptValue::Number(2.0),
                ],
            ],
        );

        let dataset = load_xpt_dataset(&path).expect("load xpt");
        let summary = dataset.summary();

        assert_eq!(dataset.metadata().name, "AE");
        assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
        assert_eq!(dataset.metadata().source_format, DatasetSourceFormat::Xpt);
        assert_eq!(dataset.metadata().variables.len(), 3);
        assert_eq!(summary.row_count, 2);
        assert_eq!(summary.columns, vec!["AESEQ", "DOMAIN", "STUDYID"]);
        assert_eq!(
            dataset
                .frame()
                .column("DOMAIN")
                .expect("domain column")
                .get(0)
                .expect("row 1")
                .extract_str(),
            Some("AE")
        );
        assert_eq!(
            dataset
                .frame()
                .column("AESEQ")
                .expect("seq column")
                .get(1)
                .expect("row 2"),
            AnyValue::Int64(2)
        );
    }

    #[test]
    fn load_xpt_dataset_preserves_zero_numeric_values() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ae.xpt");
        write_test_xpt(
            &path,
            "AE",
            &[
                TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
                TestXptVariable::numeric("AESEQ", "Sequence Number"),
            ],
            &[
                vec![TestXptValue::Text("AE"), TestXptValue::Number(0.0)],
                vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
            ],
        );

        let dataset = load_xpt_dataset(&path).expect("load xpt");
        let seq = dataset.frame().column("AESEQ").expect("seq column");

        assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(0));
        assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(1));
    }

    #[test]
    fn load_xpt_dataset_decodes_short_numeric_lengths() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ae.xpt");
        write_test_xpt(
            &path,
            "AE",
            &[
                TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
                TestXptVariable::numeric_with_length("AESEQ", 4, "Sequence Number"),
            ],
            &[
                vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
                vec![TestXptValue::Text("AE"), TestXptValue::Number(2.0)],
            ],
        );

        let dataset = load_xpt_dataset(&path).expect("load xpt");
        let seq = dataset.frame().column("AESEQ").expect("seq column");

        assert_eq!(dataset.metadata().variables[1].length, Some(4));
        assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(1));
        assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(2));
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
        write_test_xpt(
            &dir.path().join("VS.xpt"),
            "VS",
            &[
                TestXptVariable::character("STUDYID", 8, "Study Identifier"),
                TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            ],
            &[vec![TestXptValue::Text("S1"), TestXptValue::Text("VS")]],
        );

        let nested = dir.path().join("nested");
        fs::create_dir(&nested).expect("create nested");
        fs::write(nested.join("VS.csv"), "STUDYID,DOMAIN\nS1,VS\n").expect("write nested csv");

        let result = load_datasets_from_paths_with_warnings(&[dir.path().to_path_buf()])
            .expect("load directory");

        assert_eq!(result.datasets.len(), 3);
        assert_eq!(
            dataset_names(&result.datasets),
            BTreeSet::from(["AE".to_owned(), "CM".to_owned(), "VS".to_owned()])
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

    #[test]
    fn left_join_dataset_adds_prefixed_right_columns() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "DOMAIN": ["AE", "AE", "AE"],
        "AESEQ": [1, 2, 3]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S1", "S3"],
        "QNAM": ["AESPID", "AESPID"],
        "QVAL": ["A", "C"]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let datasets = load_dataset_package_json(&path).expect("load package");
        let joined = left_join_dataset(
            &datasets[0],
            &datasets[1],
            &["USUBJID".to_owned()],
            "SUPPAE.",
        )
        .expect("join datasets");

        assert_eq!(
            joined.summary().columns,
            vec!["AESEQ", "DOMAIN", "USUBJID", "SUPPAE.QNAM", "SUPPAE.QVAL"]
        );
        assert_eq!(
            joined
                .frame()
                .column("SUPPAE.QVAL")
                .expect("joined QVAL")
                .get(0)
                .expect("row 1")
                .extract_str(),
            Some("A")
        );
        assert!(joined
            .frame()
            .column("SUPPAE.QVAL")
            .expect("joined QVAL")
            .get(1)
            .expect("row 2")
            .is_null());
    }

    #[test]
    fn left_join_dataset_on_allows_different_left_and_right_key_names() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "SUBJECT": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let datasets = load_dataset_package_json(&path).expect("load package");
        let joined = left_join_dataset_on(
            &datasets[0],
            &datasets[1],
            &["USUBJID".to_owned()],
            &["SUBJECT".to_owned()],
            "LOOKUP.",
        )
        .expect("join datasets");

        assert_eq!(
            joined.summary().columns,
            vec!["DOMAIN", "USUBJID", "LOOKUP.FLAG"]
        );
        assert!(joined
            .frame()
            .column("LOOKUP.FLAG")
            .expect("joined flag")
            .get(0)
            .expect("row 1")
            .is_null());
        assert_eq!(
            joined
                .frame()
                .column("LOOKUP.FLAG")
                .expect("joined flag")
                .get(1)
                .expect("row 2")
                .extract_str(),
            Some("Y")
        );
    }

    #[test]
    fn join_variants_filter_rows_by_match_presence() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "AESEQ": [1, 2, 3]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S2", "S3"],
        "FLAG": ["Y", "N"]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let datasets = load_dataset_package_json(&path).expect("load package");
        let keys = ["USUBJID".to_owned()];
        let inner = inner_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys, "LOOKUP.")
            .expect("inner join");
        let semi =
            semi_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys).expect("semi join");
        let anti =
            anti_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys).expect("anti join");

        assert_eq!(inner.summary().row_count, 2);
        assert_eq!(semi.summary().row_count, 2);
        assert_eq!(anti.summary().row_count, 1);
        assert_eq!(
            anti.frame()
                .column("USUBJID")
                .expect("subject")
                .get(0)
                .expect("anti row")
                .extract_str(),
            Some("S1")
        );
    }

    #[test]
    fn dataset_operations_filter_derive_group_count_and_sort_rows() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S2", "S1", "S2"],
        "DOMAIN": ["AE", "AE", "AE"],
        "AESEQ": [2, 1, 3],
        "AESER": ["Y", "N", "Y"]
      }
    }
  ]
}"#,
        )
        .expect("write package");

        let datasets = load_dataset_package_json(&path).expect("load package");
        let filtered =
            filter_dataset_by_mask(&datasets[0], &[true, false, true]).expect("filter dataset");
        assert_eq!(filtered.summary().row_count, 2);

        let derived = derive_literal_column(&filtered, "SOURCE", &Value::String("TEST".to_owned()))
            .expect("derive column");
        assert_eq!(
            derived
                .frame()
                .column("SOURCE")
                .expect("source column")
                .get(0)
                .expect("source row")
                .extract_str(),
            Some("TEST")
        );

        let counted = group_count_dataset(&derived, &["USUBJID".to_owned()], "USUBJID_COUNT")
            .expect("group count");
        assert_eq!(
            counted
                .frame()
                .column("USUBJID_COUNT")
                .expect("count column")
                .get(0)
                .expect("count row"),
            AnyValue::Int64(2)
        );

        let sorted =
            sort_dataset_by_columns(&counted, &["AESEQ".to_owned()], true).expect("sort rows");
        let numbered =
            row_number_dataset(&sorted, "ROWNUM", &["USUBJID".to_owned()]).expect("row number");
        assert_eq!(
            numbered
                .frame()
                .column("AESEQ")
                .expect("seq column")
                .get(0)
                .expect("first seq"),
            AnyValue::Int64(3)
        );
        assert_eq!(
            numbered
                .frame()
                .column("ROWNUM")
                .expect("row number column")
                .get(0)
                .expect("row number"),
            AnyValue::Int64(1)
        );
    }

    #[derive(Debug, Clone)]
    struct TestXptVariable {
        name: &'static str,
        label: &'static str,
        variable_type: XptVariableType,
        length: usize,
    }

    impl TestXptVariable {
        fn character(name: &'static str, length: usize, label: &'static str) -> Self {
            Self {
                name,
                label,
                variable_type: XptVariableType::Character,
                length,
            }
        }

        fn numeric(name: &'static str, label: &'static str) -> Self {
            Self::numeric_with_length(name, 8, label)
        }

        fn numeric_with_length(name: &'static str, length: usize, label: &'static str) -> Self {
            Self {
                name,
                label,
                variable_type: XptVariableType::Numeric,
                length,
            }
        }
    }

    #[derive(Debug, Clone)]
    enum TestXptValue {
        Text(&'static str),
        Number(f64),
    }

    fn write_test_xpt(
        path: &std::path::Path,
        dataset_name: &str,
        variables: &[TestXptVariable],
        rows: &[Vec<TestXptValue>],
    ) {
        let mut bytes = Vec::new();
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        push_xpt_card(
            &mut bytes,
            "SAS     SAS     SASLIB  9.4     X64_10PRO                       18JUN26:00:00:00",
        );
        push_xpt_card(&mut bytes, "18JUN26:00:00:00");
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******MEMBER  HEADER RECORD!!!!!!!000000000000000001600000000140",
        );
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******DSCRPTR HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        push_xpt_card(
            &mut bytes,
            &format!(
                "SAS     {:<8}SASDATA 9.4     X64_10PRO                       18JUN26:00:00:00",
                dataset_name
            ),
        );
        push_xpt_card(&mut bytes, "18JUN26:00:00:00");
        push_xpt_card(
            &mut bytes,
            &format!(
                "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
                variables.len()
            ),
        );

        let mut offset = 0_u32;
        let mut namestrs = Vec::new();
        for (index, variable) in variables.iter().enumerate() {
            let mut namestr = vec![0_u8; XPT_NAMESTR_LEN];
            let ntype = match variable.variable_type {
                XptVariableType::Numeric => 1_u16,
                XptVariableType::Character => 2_u16,
            };
            namestr[0..2].copy_from_slice(&ntype.to_be_bytes());
            namestr[4..6].copy_from_slice(&(variable.length as u16).to_be_bytes());
            namestr[6..8].copy_from_slice(&((index + 1) as u16).to_be_bytes());
            write_padded(&mut namestr[8..16], variable.name);
            write_padded(&mut namestr[16..56], variable.label);
            namestr[84..88].copy_from_slice(&offset.to_be_bytes());
            offset += variable.length as u32;
            namestrs.extend(namestr);
        }
        pad_to_xpt_card(&mut namestrs);
        bytes.extend(namestrs);

        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******OBS     HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        for row in rows {
            assert_eq!(row.len(), variables.len());
            for (variable, value) in variables.iter().zip(row) {
                match (&variable.variable_type, value) {
                    (XptVariableType::Character, TestXptValue::Text(value)) => {
                        let start = bytes.len();
                        bytes.resize(start + variable.length, b' ');
                        write_padded(&mut bytes[start..start + variable.length], value);
                    }
                    (XptVariableType::Numeric, TestXptValue::Number(value)) => {
                        let encoded = f64_to_ibm_float(*value);
                        assert!(variable.length <= encoded.len());
                        bytes.extend(&encoded[..variable.length]);
                    }
                    _ => panic!("test XPT value type does not match variable type"),
                }
            }
        }
        pad_to_xpt_card(&mut bytes);

        fs::write(path, bytes).expect("write xpt");
    }

    fn push_xpt_card(bytes: &mut Vec<u8>, value: &str) {
        let start = bytes.len();
        bytes.resize(start + XPT_CARD_LEN, b' ');
        write_padded(&mut bytes[start..start + XPT_CARD_LEN], value);
    }

    fn write_padded(target: &mut [u8], value: &str) {
        let bytes = value.as_bytes();
        let len = bytes.len().min(target.len());
        target[..len].copy_from_slice(&bytes[..len]);
    }

    fn pad_to_xpt_card(bytes: &mut Vec<u8>) {
        let remainder = bytes.len() % XPT_CARD_LEN;
        if remainder != 0 {
            bytes.resize(bytes.len() + XPT_CARD_LEN - remainder, b' ');
        }
    }

    fn f64_to_ibm_float(value: f64) -> [u8; 8] {
        if value == 0.0 {
            return [0; 8];
        }

        let mut magnitude = value.abs();
        let mut exponent = 64_i32;
        while magnitude < 0.0625 {
            magnitude *= 16.0;
            exponent -= 1;
        }
        while magnitude >= 1.0 {
            magnitude /= 16.0;
            exponent += 1;
        }

        let mut output = [0_u8; 8];
        output[0] = (if value.is_sign_negative() { 0x80 } else { 0 })
            | (u8::try_from(exponent).expect("IBM exponent fits") & 0x7f);
        let fraction = (magnitude * 2_f64.powi(56)).round() as u64;
        for index in 0..7 {
            output[index + 1] = ((fraction >> (8 * (6 - index))) & 0xff) as u8;
        }
        output
    }
}
