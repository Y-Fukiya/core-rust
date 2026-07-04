use std::collections::{BTreeSet, HashMap, HashSet};

use polars::prelude::*;
use serde_json::Value;

use super::{
    cell_to_json_value, row_key, series_from_json_values, take_dataset_rows, DataError,
    LoadedDataset, Result, RowKeyValue,
};

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
