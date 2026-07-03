use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use indexmap::IndexMap;
use polars::prelude::*;
use serde_json::Value;

use crate::{
    canonical_or_original, column_names, DataError, DatasetMetadata, DatasetSourceFormat,
    DatasetVariable, LoadedDataset, Result,
};

pub(crate) fn json_rows_dataset(
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

pub(crate) fn records_to_frame(records: &IndexMap<String, Vec<Value>>) -> PolarsResult<DataFrame> {
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

pub(crate) fn series_from_json_values(name: &str, values: &[Value]) -> Series {
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
