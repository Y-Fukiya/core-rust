use std::collections::{BTreeMap, BTreeSet};

use core_data::{dataset_column_values, filter_dataset_by_mask, DataError, LoadedDataset};
use core_engine::{evaluate_condition_group, RuleValidationResult};
use core_rule_model::{
    normalize_condition_value, Condition, ConditionGroup, ExecutableRule, OperationSpec, Operator,
    RuleModelError, ValueExpr,
};
use serde_json::Value;

use crate::json_values::json_scalar_string;
use crate::operation_fields::{
    clean_operation_identifier, normalize_operation_key, operation_value, string_field,
};
use crate::{
    derive_column_from_values_with_aliases, expand_dataset_domain_placeholder,
    operation_skipped_result,
};

pub(crate) fn operation_column_values(
    dataset: &LoadedDataset,
    column_name: &str,
) -> std::result::Result<Vec<Value>, DataError> {
    let resolved = resolve_operation_column_name(dataset, column_name)
        .unwrap_or_else(|| column_name.to_owned());
    dataset_column_values(dataset, &resolved)
}

pub(crate) fn operation_group_key_columns(
    dataset: &LoadedDataset,
    keys: &[String],
) -> std::result::Result<Vec<Vec<Value>>, DataError> {
    let mut columns = Vec::new();
    for key in keys {
        let key = expand_dataset_domain_placeholder(dataset, key);
        let values = operation_column_values(dataset, &key)?;
        if let Some(variable_names) = dynamic_column_list_values(&values) {
            for variable_name in variable_names {
                columns.push(operation_column_values(dataset, &variable_name)?);
            }
        } else {
            columns.push(values);
        }
    }
    Ok(columns)
}

fn dynamic_column_list_values(values: &[Value]) -> Option<Vec<String>> {
    values.iter().find_map(|value| {
        json_scalar_string(value)
            .as_deref()
            .and_then(parse_column_list_literal)
            .filter(|columns| !columns.is_empty())
    })
}

fn parse_column_list_literal(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let columns = inner
        .split(',')
        .filter_map(|part| {
            let column = part.trim().trim_matches('"').trim_matches('\'').trim();
            (!column.is_empty()).then(|| column.to_owned())
        })
        .collect::<Vec<_>>();
    Some(columns)
}

fn resolve_operation_column_name(dataset: &LoadedDataset, column_name: &str) -> Option<String> {
    let clean_target = clean_operation_identifier(column_name);
    let columns = dataset.summary().columns;
    if let Some(found) = columns.iter().find(|candidate| {
        candidate.as_str() == column_name
            || candidate.eq_ignore_ascii_case(column_name)
            || clean_operation_identifier(candidate) == clean_target
    }) {
        return Some(found.clone());
    }
    let base_name = column_name.rsplit_once('.').map(|(base, _suffix)| base)?;
    let clean_base = clean_operation_identifier(base_name);
    columns.into_iter().find(|candidate| {
        candidate == base_name
            || candidate.eq_ignore_ascii_case(base_name)
            || clean_operation_identifier(candidate) == clean_base
    })
}

pub(crate) fn group_count_dataset_with_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
    keys: &[String],
    output: &str,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let regex_pattern = string_field(operation, &["regex", "pattern"]);
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        let mask = vec![true; dataset.summary().row_count];
        return derive_filtered_group_count_dataset(
            dataset,
            &mask,
            keys,
            output,
            regex_pattern.as_deref(),
        )
        .map_err(|source| operation_skipped_result(rule, source.to_string()));
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    derive_filtered_group_count_dataset(dataset, &mask, keys, output, regex_pattern.as_deref())
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

fn derive_filtered_group_count_dataset(
    dataset: &LoadedDataset,
    mask: &[bool],
    keys: &[String],
    column_name: &str,
    regex_pattern: Option<&str>,
) -> std::result::Result<LoadedDataset, DataError> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "group count operation requires an output column".to_owned(),
        ));
    }
    let row_count = dataset.summary().row_count;
    if mask.len() != row_count {
        return Err(DataError::InvalidDatasetPackage(format!(
            "filter mask length {} does not match row count {}",
            mask.len(),
            row_count
        )));
    }

    if keys.is_empty() {
        let count = mask.iter().filter(|keep| **keep).count() as i64;
        let values = (0..row_count)
            .map(|_| Value::Number(serde_json::Number::from(count)))
            .collect::<Vec<_>>();
        return derive_column_from_values_with_aliases(dataset, column_name, &values);
    }

    let key_columns = operation_group_key_columns(dataset, keys)?;
    let regex = regex_pattern
        .map(regex::Regex::new)
        .transpose()
        .map_err(|source| DataError::InvalidDatasetPackage(source.to_string()))?;
    let mut counts = BTreeMap::new();
    for (row, keep) in mask.iter().enumerate().take(row_count) {
        if *keep {
            *counts
                .entry(filtered_group_count_key(&key_columns, row, regex.as_ref()))
                .or_insert(0_i64) += 1;
        }
    }
    let values = (0..row_count)
        .map(|row| {
            let count = *counts
                .get(&filtered_group_count_key(&key_columns, row, regex.as_ref()))
                .unwrap_or(&0_i64);
            Value::Number(serde_json::Number::from(count))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn filtered_group_count_key(
    columns: &[Vec<Value>],
    row: usize,
    regex: Option<&regex::Regex>,
) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            let value = column
                .get(row)
                .and_then(json_scalar_string)
                .unwrap_or_default();
            normalize_operation_group_key_value(&value, regex)
        })
        .collect()
}

fn normalize_operation_group_key_value(value: &str, regex: Option<&regex::Regex>) -> String {
    regex
        .and_then(|regex| regex.find(value))
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| value.to_owned())
}

pub(crate) fn group_distinct_values_dataset_with_aliases(
    dataset: &LoadedDataset,
    keys: &[String],
    aliases: &[String],
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
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
    if !aliases.is_empty() && aliases.len() != keys.len() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires matching group and group_aliases".to_owned(),
        ));
    }

    let source_key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(dataset, key).map_err(|_source| {
                DataError::InvalidDatasetPackage(format!("distinct values key not found: {key}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let target_keys = if aliases.is_empty() { keys } else { aliases };
    let target_key_columns = target_keys
        .iter()
        .map(|key| {
            operation_column_values(dataset, key).map_err(|_source| {
                DataError::InvalidDatasetPackage(format!("distinct values key not found: {key}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let source_values = operation_column_values(dataset, source_column).map_err(|_source| {
        DataError::InvalidDatasetPackage(format!(
            "distinct values source column not found: {source_column}"
        ))
    })?;

    let mut groups = BTreeMap::<Vec<String>, BTreeSet<String>>::new();
    for row in 0..dataset.frame().height() {
        if let Some(value) = source_values.get(row).and_then(json_scalar_string) {
            groups
                .entry(filtered_group_count_key(&source_key_columns, row, None))
                .or_default()
                .insert(value);
        }
    }

    let values = (0..dataset.frame().height())
        .map(|row| {
            let joined = groups
                .get(&filtered_group_count_key(&target_key_columns, row, None))
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default();
            Value::String(joined)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn apply_operation_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let mask = operation_inline_filter_mask(rule, operation, dataset)?;
    filter_dataset_by_mask(dataset, &mask)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

pub(crate) fn operation_inline_filter_mask(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
) -> std::result::Result<Vec<bool>, RuleValidationResult> {
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        return Ok(vec![true; dataset.summary().row_count]);
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    Ok(mask)
}

fn normalize_operation_filter_value(
    value: &Value,
) -> std::result::Result<ConditionGroup, RuleModelError> {
    let Some(object) = value.as_object() else {
        return normalize_condition_value(value);
    };
    if object.keys().any(|key| {
        matches!(
            normalize_operation_key(key).as_str(),
            "all" | "any" | "not" | "name" | "target" | "operator"
        )
    }) {
        return normalize_condition_value(value);
    }

    Ok(ConditionGroup::All(
        object
            .iter()
            .map(|(target, comparator)| {
                ConditionGroup::Leaf(Condition {
                    target: Some(target.clone()),
                    operator: Operator::EqualTo,
                    comparator: ValueExpr::Literal(comparator.clone()),
                    options: Default::default(),
                })
            })
            .collect(),
    ))
}
