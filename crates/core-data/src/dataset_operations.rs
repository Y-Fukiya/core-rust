use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use polars::prelude::*;
use serde_json::Value;

use crate::{
    cell_to_json_value, cell_to_string, column_names, number_value, row_key,
    series_from_json_values, take_dataset_rows, DataError, LoadedDataset, Result, RowKeyValue,
};

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
        .map(|row| cell_to_json_value(&dataset.frame, source_column, row))
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
        .map(|row| cell_to_json_value(&dataset.frame, column_name, row))
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

    let mut groups: HashMap<Vec<RowKeyValue>, GroupAccumulator> = HashMap::new();
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

pub fn group_distinct_values_dataset(
    dataset: &LoadedDataset,
    keys: &[String],
    source_column: &str,
    column_name: &str,
) -> Result<LoadedDataset> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires an output column".to_owned(),
        ));
    }
    for key in keys {
        if dataset.frame.column(key).is_err() {
            return Err(DataError::InvalidDatasetPackage(format!(
                "distinct values key not found: {key}"
            )));
        }
    }
    if dataset.frame.column(source_column).is_err() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "distinct values source column not found: {source_column}"
        )));
    }

    let mut groups: HashMap<Vec<RowKeyValue>, BTreeSet<String>> = HashMap::new();
    for row in 0..dataset.frame.height() {
        if let Some(value) = cell_to_string(&dataset.frame, source_column, row)? {
            groups
                .entry(row_key(&dataset.frame, keys, row)?)
                .or_default()
                .insert(value);
        }
    }

    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = row_key(&dataset.frame, keys, row)?;
            let joined = groups
                .get(&key)
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default();
            Ok(Value::String(joined))
        })
        .collect::<Result<Vec<_>>>()?;
    derive_column_from_values(dataset, column_name, &values)
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

    let mut counters: HashMap<Vec<RowKeyValue>, i64> = HashMap::new();
    let values = (0..dataset.frame.height())
        .map(|row| {
            let key = if keys.is_empty() {
                vec![RowKeyValue::String("<ALL>".to_owned())]
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
    if keep.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "drop operation cannot remove all columns".to_owned(),
        ));
    }
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
