use std::collections::{BTreeMap, BTreeSet};

use core_data::{
    derive_column_from_column, derive_literal_column, left_join_dataset_on, rename_dataset_columns,
    row_number_dataset, LoadedDataset,
};
use core_engine::RuleValidationResult;
use core_rule_model::{ConditionGroup, ExecutableRule, MatchDataset, ValueExpr};
use serde_json::Value;

use crate::operation_fields::normalize_operation_key;
use crate::{
    dataset_column_name, dataset_has_column, dataset_matches_name,
    expand_dataset_domain_placeholder, find_dataset, join_skipped_result,
};

const SOURCE_ROW_COLUMN: &str = "__core_source_row";

pub(crate) fn execute_match_datasets(
    rule: &ExecutableRule,
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let match_datasets = rule.datasets.as_deref().unwrap_or_default();
    let mut names = match_datasets
        .iter()
        .filter_map(match_dataset_name)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(join_skipped_result(
            rule,
            "Match Datasets is missing dataset names",
        ));
    }
    if rule.entities.is_some() && !scoped_datasets.is_empty() {
        return execute_scoped_match_dataset_sequence(
            rule,
            match_datasets,
            &names,
            scoped_datasets,
            all_datasets,
        );
    }
    if names.len() == 1 {
        return execute_single_match_dataset(
            rule,
            &match_datasets[0],
            &names[0],
            scoped_datasets,
            all_datasets,
        );
    }

    let left_name = names.remove(0);
    let Some(mut joined) = find_dataset(all_datasets, &left_name).cloned() else {
        return Err(join_skipped_result(
            rule,
            format!("left dataset {left_name} was not loaded"),
        ));
    };

    for (index, right_name) in names.iter().enumerate() {
        let Some(right) = find_dataset(all_datasets, right_name) else {
            return Err(join_skipped_result(
                rule,
                format!("right dataset {right_name} was not loaded"),
            ));
        };
        let keys = match_datasets
            .get(index + 1)
            .and_then(match_dataset_join_keys)
            .or_else(|| match_datasets.first().and_then(match_dataset_join_keys))
            .or_else(|| common_join_keys(&joined, right).map(JoinKeys::same));
        let Some(keys) = keys else {
            return Err(join_skipped_result(
                rule,
                format!("no common join keys for {left_name} and {right_name}"),
            ));
        };
        let prefix = match_datasets
            .get(index + 1)
            .and_then(|dataset| match_dataset_string_field(dataset, &["prefix"]))
            .unwrap_or_else(|| format!("{right_name}."));
        joined = left_join_dataset_on(&joined, right, &keys.left, &keys.right, &prefix)
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
    }

    Ok(vec![joined])
}

fn execute_scoped_match_dataset_sequence(
    rule: &ExecutableRule,
    match_datasets: &[MatchDataset],
    names: &[String],
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let mut joined_datasets = Vec::with_capacity(scoped_datasets.len());
    for scoped_base in scoped_datasets {
        let mut joined = scoped_base.clone();
        for (match_dataset, match_name) in match_datasets.iter().zip(names) {
            let Some(lookup_dataset) = find_dataset(all_datasets, match_name) else {
                if is_left_match_dataset(match_dataset) {
                    joined = add_missing_left_match_columns(&joined, rule, match_name)
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
                    continue;
                }
                return Err(join_skipped_result(
                    rule,
                    format!("dataset {match_name} was not loaded"),
                ));
            };
            let keys = match_dataset_join_keys(match_dataset)
                .or_else(|| common_join_keys(&joined, lookup_dataset).map(JoinKeys::same));
            let Some(keys) = keys else {
                return Err(join_skipped_result(
                    rule,
                    format!("match dataset {match_name} is missing keys"),
                ));
            };

            if let Some(prefix) = match_dataset_string_field(match_dataset, &["prefix"]) {
                joined =
                    left_join_dataset_on(&joined, lookup_dataset, &keys.left, &keys.right, &prefix)
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
            } else {
                let lookup_dataset = suffix_conflicting_match_columns(
                    &joined,
                    lookup_dataset,
                    &keys.right,
                    match_name,
                    rule,
                )
                .map_err(|source| join_skipped_result(rule, source.to_string()))?;
                joined =
                    left_join_dataset_on(&joined, &lookup_dataset, &keys.left, &keys.right, "")
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
            }
        }
        joined_datasets.push(joined);
    }
    Ok(joined_datasets)
}

fn suffix_conflicting_match_columns(
    left: &LoadedDataset,
    right: &LoadedDataset,
    right_keys: &[String],
    suffix: &str,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    let mut right = right.clone();
    for key in right_keys {
        if !dataset_has_column(left, key) {
            continue;
        }
        let suffixed_key = format!("{key}.{suffix}");
        if !rule_references_column(rule, &suffixed_key) {
            continue;
        }
        if dataset_has_column(left, &suffixed_key) || dataset_has_column(&right, &suffixed_key) {
            continue;
        }
        let Some(source_key) = dataset_column_name(&right, key) else {
            continue;
        };
        right = derive_column_from_column(&right, &suffixed_key, &source_key)?;
    }

    let mut renames = BTreeMap::new();
    for column in right.frame().get_column_names() {
        let column = column.as_str();
        if right_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(column))
        {
            continue;
        }
        let suffixed_column = format!("{column}.{suffix}");
        if dataset_has_column(left, column) || rule_references_column(rule, &suffixed_column) {
            renames.insert(column.to_owned(), suffixed_column);
        }
    }
    if renames.is_empty() {
        return Ok(right);
    }
    rename_dataset_columns(&right, &renames)
}

fn add_missing_left_match_columns(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
    suffix: &str,
) -> core_data::Result<LoadedDataset> {
    let mut joined = dataset.clone();
    for column in rule_referenced_columns_with_suffix(rule, suffix) {
        if dataset_has_column(&joined, &column) {
            continue;
        }
        joined = derive_literal_column(&joined, &column, &Value::Null)?;
    }
    Ok(joined)
}

pub(crate) fn rule_referenced_columns_with_suffix(
    rule: &ExecutableRule,
    suffix: &str,
) -> BTreeSet<String> {
    let mut columns = BTreeSet::new();
    for variable in &rule.output_variables {
        collect_column_with_suffix(variable, suffix, &mut columns);
    }
    collect_condition_columns_with_suffix(&rule.conditions, suffix, &mut columns);
    columns
}

fn collect_condition_columns_with_suffix(
    group: &ConditionGroup,
    suffix: &str,
    columns: &mut BTreeSet<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_condition_columns_with_suffix(group, suffix, columns);
            }
        }
        ConditionGroup::Not(group) => collect_condition_columns_with_suffix(group, suffix, columns),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = condition.target.as_deref() {
                collect_column_with_suffix(target, suffix, columns);
            }
            collect_value_expr_columns_with_suffix(&condition.comparator, suffix, columns);
        }
    }
}

fn collect_value_expr_columns_with_suffix(
    expr: &ValueExpr,
    suffix: &str,
    columns: &mut BTreeSet<String>,
) {
    match expr {
        ValueExpr::ColumnRef(reference) => collect_column_with_suffix(reference, suffix, columns),
        ValueExpr::List(values) => {
            for value in values {
                if let Some(reference) = value.as_str() {
                    collect_column_with_suffix(reference, suffix, columns);
                }
            }
        }
        ValueExpr::Literal(_) | ValueExpr::Null => {}
    }
}

fn collect_column_with_suffix(column: &str, suffix: &str, columns: &mut BTreeSet<String>) {
    if column
        .rsplit_once('.')
        .is_some_and(|(_, column_suffix)| column_suffix.eq_ignore_ascii_case(suffix))
    {
        columns.insert(column.to_owned());
    }
}

fn rule_references_column(rule: &ExecutableRule, column: &str) -> bool {
    rule.output_variables
        .iter()
        .any(|variable| variable.eq_ignore_ascii_case(column))
        || condition_group_references_column(&rule.conditions, column)
}

fn condition_group_references_column(group: &ConditionGroup, column: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| condition_group_references_column(group, column)),
        ConditionGroup::Not(group) => condition_group_references_column(group, column),
        ConditionGroup::Leaf(condition) => {
            condition
                .target
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case(column))
                || value_expr_references_column(&condition.comparator, column)
        }
    }
}

fn value_expr_references_column(expr: &ValueExpr, column: &str) -> bool {
    match expr {
        ValueExpr::ColumnRef(reference) => reference.eq_ignore_ascii_case(column),
        ValueExpr::List(values) => values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|reference| reference.eq_ignore_ascii_case(column))
        }),
        ValueExpr::Literal(_) | ValueExpr::Null => false,
    }
}

fn execute_single_match_dataset(
    rule: &ExecutableRule,
    match_dataset: &MatchDataset,
    match_name: &str,
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let scoped_bases = scoped_datasets
        .iter()
        .filter(|dataset| {
            crate::engine_semantics::includes_single_match_dataset_as_target(rule)
                || !dataset_matches_name(dataset, match_name)
        })
        .collect::<Vec<_>>();
    if scoped_bases.is_empty() {
        let Some(dataset) = find_dataset(scoped_datasets, match_name) else {
            return Err(join_skipped_result(
                rule,
                format!("dataset {match_name} was not loaded"),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    let Some(lookup_dataset) = find_dataset(all_datasets, match_name) else {
        if is_left_match_dataset(match_dataset) {
            return Ok(scoped_bases.into_iter().cloned().collect());
        }
        return Err(join_skipped_result(
            rule,
            format!("dataset {match_name} was not loaded"),
        ));
    };
    let Some(keys) = match_dataset_join_keys(match_dataset) else {
        return Err(join_skipped_result(
            rule,
            format!("match dataset {match_name} is missing keys"),
        ));
    };
    let prefix = match_dataset_string_field(match_dataset, &["prefix"])
        .unwrap_or_else(|| default_single_match_dataset_prefix(rule, match_name));
    let mut joined_datasets = Vec::with_capacity(scoped_bases.len());
    for scoped_base in scoped_bases {
        if !dataset_has_join_keys(scoped_base, &keys.left) {
            continue;
        }
        let scoped_base = add_source_row_column(scoped_base)
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        let lookup_dataset = if prefix.is_empty() {
            suffix_conflicting_match_columns(
                &scoped_base,
                lookup_dataset,
                &keys.right,
                match_name,
                rule,
            )
            .map_err(|source| join_skipped_result(rule, source.to_string()))?
        } else {
            lookup_dataset.clone()
        };
        let mut joined = left_join_dataset_on(
            &scoped_base,
            &lookup_dataset,
            &keys.left,
            &keys.right,
            &prefix,
        )
        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        if !prefix.is_empty() {
            joined = add_unprefixed_match_aliases(
                &joined,
                &scoped_base,
                &lookup_dataset,
                &keys.right,
                &prefix,
                rule,
            )
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        }
        joined_datasets.push(joined);
    }
    Ok(joined_datasets)
}

fn dataset_has_join_keys(dataset: &LoadedDataset, keys: &[String]) -> bool {
    keys.iter()
        .map(|key| expand_dataset_domain_placeholder(dataset, key))
        .all(|key| dataset_has_column(dataset, &key))
}

pub(crate) fn add_source_row_column(dataset: &LoadedDataset) -> core_data::Result<LoadedDataset> {
    if dataset_has_column(dataset, SOURCE_ROW_COLUMN) {
        return Ok(dataset.clone());
    }
    row_number_dataset(dataset, SOURCE_ROW_COLUMN, &[])
}

fn add_unprefixed_match_aliases(
    joined: &LoadedDataset,
    left: &LoadedDataset,
    right: &LoadedDataset,
    right_keys: &[String],
    prefix: &str,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    let mut joined = joined.clone();
    for column in right.frame().get_column_names() {
        let column = column.as_str();
        if right_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(column))
            || dataset_has_column(left, column)
            || dataset_has_column(&joined, column)
            || !rule_references_column(rule, column)
        {
            continue;
        }
        let prefixed_column = format!("{prefix}{column}");
        if !dataset_has_column(&joined, &prefixed_column) {
            continue;
        }
        joined = derive_column_from_column(&joined, column, &prefixed_column)?;
    }
    Ok(joined)
}

fn default_single_match_dataset_prefix(rule: &ExecutableRule, match_name: &str) -> String {
    if rule_references_match_dataset_prefixed_column(rule, match_name) {
        format!("{match_name}.")
    } else {
        String::new()
    }
}

pub(crate) fn rule_references_match_dataset_prefixed_column(
    rule: &ExecutableRule,
    match_name: &str,
) -> bool {
    rule.output_variables
        .iter()
        .any(|variable| column_has_match_dataset_prefix(variable, match_name))
        || condition_group_references_match_dataset_prefix(&rule.conditions, match_name)
}

fn condition_group_references_match_dataset_prefix(
    group: &ConditionGroup,
    match_name: &str,
) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| condition_group_references_match_dataset_prefix(group, match_name)),
        ConditionGroup::Not(group) => {
            condition_group_references_match_dataset_prefix(group, match_name)
        }
        ConditionGroup::Leaf(condition) => {
            condition
                .target
                .as_deref()
                .is_some_and(|target| column_has_match_dataset_prefix(target, match_name))
                || value_expr_references_match_dataset_prefix(&condition.comparator, match_name)
        }
    }
}

fn value_expr_references_match_dataset_prefix(expr: &ValueExpr, match_name: &str) -> bool {
    match expr {
        ValueExpr::ColumnRef(reference) => column_has_match_dataset_prefix(reference, match_name),
        ValueExpr::List(values) => values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|reference| column_has_match_dataset_prefix(reference, match_name))
        }),
        ValueExpr::Literal(_) | ValueExpr::Null => false,
    }
}

fn column_has_match_dataset_prefix(column: &str, match_name: &str) -> bool {
    column
        .split_once('.')
        .is_some_and(|(prefix, _)| prefix.eq_ignore_ascii_case(match_name))
}

pub(crate) fn match_dataset_name(dataset: &MatchDataset) -> Option<String> {
    match_dataset_string_field(
        dataset,
        &[
            "dataset", "domain", "name", "id", "Dataset", "Domain", "Name",
        ],
    )
}

fn is_left_match_dataset(dataset: &MatchDataset) -> bool {
    match_dataset_string_field(dataset, &["join_type", "join type", "Join Type"])
        .is_some_and(|join_type| join_type.eq_ignore_ascii_case("left"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JoinKeys {
    left: Vec<String>,
    right: Vec<String>,
}

impl JoinKeys {
    fn same(keys: Vec<String>) -> Self {
        Self {
            left: keys.clone(),
            right: keys,
        }
    }
}

fn match_dataset_join_keys(dataset: &MatchDataset) -> Option<JoinKeys> {
    let value = match_dataset_value(dataset, &["by", "keys", "on", "join_keys", "match_keys"])?;
    join_keys_from_value(value)
}

fn join_keys_from_value(value: &Value) -> Option<JoinKeys> {
    match value {
        Value::String(value) if !value.is_empty() => Some(JoinKeys::same(vec![value.clone()])),
        Value::Array(values) => {
            let mut left = Vec::new();
            let mut right = Vec::new();
            for value in values {
                match value {
                    Value::String(value) if !value.is_empty() => {
                        left.push(value.clone());
                        right.push(value.clone());
                    }
                    Value::Object(_) => {
                        let left_key = object_string_field(value, &["left"])?;
                        let right_key = object_string_field(value, &["right"])?;
                        left.push(left_key);
                        right.push(right_key);
                    }
                    _ => return None,
                }
            }
            (!left.is_empty()).then_some(JoinKeys { left, right })
        }
        Value::Object(_) => {
            let left = object_string_field(value, &["left"])?;
            let right = object_string_field(value, &["right"])?;
            Some(JoinKeys {
                left: vec![left],
                right: vec![right],
            })
        }
        _ => None,
    }
}

fn object_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    let Value::Object(fields) = value else {
        return None;
    };
    keys.iter().find_map(|key| {
        fields
            .get(*key)
            .or_else(|| {
                let normalized = normalize_operation_key(key);
                fields
                    .iter()
                    .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized)
                    .map(|(_key, value)| value)
            })
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn match_dataset_string_field(dataset: &MatchDataset, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| match_dataset_value(dataset, &[*key]))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn match_dataset_value<'a>(dataset: &'a MatchDataset, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        dataset
            .fields
            .get(*key)
            .or_else(|| match_dataset_field_normalized(dataset, key))
    })
}

fn match_dataset_field_normalized<'a>(dataset: &'a MatchDataset, key: &str) -> Option<&'a Value> {
    let normalized_key = normalize_operation_key(key);
    dataset
        .fields
        .iter()
        .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized_key)
        .map(|(_key, value)| value)
}

fn common_join_keys(left: &LoadedDataset, right: &LoadedDataset) -> Option<Vec<String>> {
    let right_columns = right
        .summary()
        .columns
        .into_iter()
        .map(|column| column.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let left_columns = left.summary().columns;
    let mut keys = Vec::new();
    for preferred in ["STUDYID", "USUBJID", "DOMAIN", "IDVAR", "IDVARVAL"] {
        if left_columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case(preferred))
            && right_columns.contains(&preferred.to_ascii_lowercase())
        {
            keys.push(preferred.to_owned());
        }
    }
    for column in left_columns {
        if right_columns.contains(&column.to_ascii_lowercase())
            && !keys.iter().any(|key| key.eq_ignore_ascii_case(&column))
        {
            keys.push(column);
        }
    }
    (!keys.is_empty()).then_some(keys)
}
