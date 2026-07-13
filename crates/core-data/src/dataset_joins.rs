use std::collections::{BTreeSet, HashMap, HashSet};

use super::{
    row_key, row_key_contains_null, take_dataset_rows, DataError, LoadedDataset, Result,
    RowKeyValue,
};
use polars::prelude::*;

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
        let key = row_key(&right.frame, &right_keys, row)?;
        if !row_key_contains_null(&key) {
            index.entry(key).or_default().push(row);
        }
    }

    let mut left_rows = Vec::new();
    let mut right_rows = Vec::new();
    for left_row in 0..left.frame.height() {
        let key = row_key(&left.frame, &left_keys, left_row)?;
        if !row_key_contains_null(&key) {
            if let Some(matches) = index.get(&key) {
                for right_row in matches {
                    left_rows.push(left_row as u32);
                    right_rows.push(Some(*right_row));
                }
                continue;
            }
        }
        if include_unmatched_left {
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

    let right_indices = UInt32Chunked::from_iter_options(
        "right_row_index".into(),
        right_rows.iter().map(|row| row.map(|row| row as u32)),
    );
    let right_frame = right
        .frame
        .take(&right_indices)
        .map_err(|source| DataError::Polars {
            path: right.metadata.full_path.clone(),
            source,
        })?;

    let mut joined_names = BTreeSet::new();
    let mut joined_columns = Vec::new();
    for right_column in right.frame.get_column_names() {
        let right_column = right_column.as_str();
        if !right_prefix.is_empty() && right_keys.iter().any(|key| key == right_column) {
            continue;
        }

        let joined_name = format!("{right_prefix}{right_column}");
        if right_keys.iter().any(|key| key == right_column) && left_columns.contains(&joined_name) {
            continue;
        }
        let joined_name =
            deduplicate_joined_column_name(&joined_name, &left_columns, &joined_names);
        let mut column = right_frame
            .column(right_column)
            .map_err(|source| DataError::Polars {
                path: right.metadata.full_path.clone(),
                source,
            })?
            .clone();
        column.rename(joined_name.clone().into());

        joined_names.insert(joined_name);
        joined_columns.push(column);
    }

    frame
        .hstack_mut(&joined_columns)
        .map_err(|source| DataError::Polars {
            path: left.metadata.full_path.clone(),
            source,
        })?;

    Ok(LoadedDataset::new(left.metadata.clone(), frame))
}

fn deduplicate_joined_column_name(
    requested: &str,
    left_columns: &BTreeSet<String>,
    joined_columns: &BTreeSet<String>,
) -> String {
    if !left_columns.contains(requested) && !joined_columns.contains(requested) {
        return requested.to_owned();
    }
    let base = format!("{requested}_right");
    if !left_columns.contains(&base) && !joined_columns.contains(&base) {
        return base;
    }
    for index in 2.. {
        let candidate = format!("{base}_{index}");
        if !left_columns.contains(&candidate) && !joined_columns.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded join column suffix search must return")
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
        let key = row_key(&right.frame, &right_keys, row)?;
        if !row_key_contains_null(&key) {
            index.insert(key);
        }
    }

    let indices = (0..left.frame.height())
        .filter_map(|row| {
            row_key(&left.frame, &left_keys, row)
                .map(|key| {
                    let matches = !row_key_contains_null(&key) && index.contains(&key);
                    (matches == keep_matches).then_some(row as u32)
                })
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
