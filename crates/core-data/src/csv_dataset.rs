use std::collections::BTreeMap;
use std::path::Path;

use polars::prelude::*;
use serde_json::Value;

use crate::dataset_paths::{canonical_or_original, column_names, file_name, file_stem};
use crate::json_table::series_from_json_values;
use crate::{
    cell_to_string, DataError, DatasetMetadata, DatasetSourceFormat, DatasetVariable,
    LoadedDataset, Result,
};

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

pub(crate) fn parse_csv_bool(value: &str) -> Option<bool> {
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

fn number_value(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}
