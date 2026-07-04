use std::cmp::Ordering;
use std::path::PathBuf;

use polars::prelude::*;

use crate::{DataError, Result};

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
pub(crate) struct NumberKey(u64);

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
