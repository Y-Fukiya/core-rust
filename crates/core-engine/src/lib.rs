#![forbid(unsafe_code)]

use core_data::LoadedDataset;
use core_rule_model::{
    ActionSpec, Condition, ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity,
    ValueExpr,
};
use polars::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use thiserror::Error;

mod date_operators;
mod group_operators;
mod scalar_operators;

use date_operators::{
    classify_date_value, compare_complete_dates, is_incomplete_date, is_valid_iso_duration,
    parse_complete_date, DateValueState,
};
use group_operators::{
    evaluate_inconsistent_across_dataset, evaluate_inconsistent_enumerated_columns,
    evaluate_not_unique_relationship, evaluate_unique_set,
};
#[cfg(test)]
use scalar_operators::scalar_contained_by_value;
use scalar_operators::{
    expand_domain_placeholder, json_value_to_scalar, resolve_scalar_comparator,
    resolve_scalar_list_comparator, scalar_contains_all, scalar_equal_with_mode,
    scalar_is_ordered_subset_of, scalar_matches_comparator, scalar_shares_no_elements_with,
    string_contains_value, string_prefix, string_suffix, ScalarValue,
};

pub type Result<T> = std::result::Result<T, EngineError>;
pub type BooleanMask = Vec<bool>;

const SOURCE_ROW_COLUMN: &str = "__core_source_row";

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("condition is missing a target column")]
    MissingTarget,
    #[error("condition using operator {operator} is missing a comparator")]
    MissingComparator { operator: String },
    #[error("rule is missing sensitivity")]
    MissingSensitivity,
    #[error("dataset is missing required column: {0}")]
    MissingColumn(String),
    #[error("unsupported rule sensitivity: {0}")]
    UnsupportedSensitivity(String),
    #[error("unsupported operator: {0}")]
    UnsupportedOperator(String),
    #[error("operator {operator} cannot use comparator {comparator:?}")]
    InvalidComparator {
        operator: String,
        comparator: ValueExpr,
    },
    #[error("failed to evaluate Polars data: {0}")]
    Polars(#[from] PolarsError),
    #[error("invalid regex pattern: {0}")]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    RuleNotFound,
    StandardMismatch,
    UnsupportedRuleType,
    UnsupportedOperator,
    OperationsNotSupported,
    OracleSemanticsGap,
    JsonataNotSupported,
    DatasetJoinNotSupported,
    EvaluationError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleValidationResult {
    pub rule_id: String,
    pub execution_status: ExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_provenance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<SkippedReason>,
    pub dataset: String,
    pub domain: Option<String>,
    pub message: String,
    pub error_count: usize,
    pub errors: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationIssue {
    pub rule_id: String,
    pub dataset: String,
    pub domain: Option<String>,
    pub row: Option<usize>,
    pub variables: Vec<String>,
    pub message: String,
    pub usubjid: Option<String>,
    pub seq: Option<String>,
}

impl RuleValidationResult {
    pub fn skipped_rule(
        rule_id: impl Into<String>,
        reason: SkippedReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            execution_status: ExecutionStatus::Skipped,
            execution_provenance: None,
            skipped_reason: Some(reason),
            dataset: String::new(),
            domain: None,
            message: message.into(),
            error_count: 0,
            errors: Vec::new(),
        }
    }
}

pub fn validate_rule(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> Result<RuleValidationResult> {
    let dataset_presence_exists = (matches!(rule.sensitivity.as_ref(), Some(Sensitivity::Dataset))
        && matches!(
            rule.rule_type,
            RuleType::RecordData | RuleType::DomainPresence
        ))
        || contains_relationship_operator(&rule.conditions)
        || contains_unique_set_operator(&rule.conditions);
    let mask =
        evaluate_condition_group_with_options(&rule.conditions, dataset, dataset_presence_exists)?;
    let message =
        outcome_message(&rule.actions).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let variables = issue_variables(rule, dataset);

    let errors = match rule
        .sensitivity
        .as_ref()
        .ok_or(EngineError::MissingSensitivity)?
    {
        Sensitivity::Record => record_level_issues(rule, dataset, &mask, &variables, &message)?,
        Sensitivity::Group => group_level_issues(rule, dataset, &mask, &variables, &message)?,
        Sensitivity::Dataset
            if matches!(
                rule.rule_type,
                RuleType::RecordData | RuleType::DomainPresence
            ) =>
        {
            record_level_issues(rule, dataset, &mask, &variables, &message)?
        }
        Sensitivity::Dataset => dataset_level_issues(rule, dataset, &mask, &variables, &message),
        sensitivity => {
            return Err(EngineError::UnsupportedSensitivity(
                sensitivity.as_name().to_owned(),
            ))
        }
    };
    let error_count = errors.len();

    Ok(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: if error_count == 0 {
            ExecutionStatus::Passed
        } else {
            ExecutionStatus::Failed
        },
        execution_provenance: None,
        skipped_reason: None,
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message,
        error_count,
        errors,
    })
}

pub fn evaluate_rule_conditions(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> Result<BooleanMask> {
    evaluate_condition_group(&rule.conditions, dataset)
}

pub fn evaluate_condition_group(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
) -> Result<BooleanMask> {
    evaluate_condition_group_with_options(group, dataset, false)
}

fn evaluate_condition_group_with_options(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
    dataset_presence_exists: bool,
) -> Result<BooleanMask> {
    let row_count = dataset.frame().height();

    match group {
        ConditionGroup::All(groups) => {
            let mut mask = vec![true; row_count];
            for group in groups {
                match evaluate_condition_group_with_options(group, dataset, dataset_presence_exists)
                {
                    Ok(branch) => and_assign(&mut mask, &branch),
                    Err(EngineError::MissingColumn(_))
                        if missing_column_is_false_in_all_group(group) =>
                    {
                        mask.fill(false);
                    }
                    Err(source) => return Err(source),
                }
                if mask.iter().all(|value| !*value) {
                    break;
                }
            }
            Ok(mask)
        }
        ConditionGroup::Any(groups) => {
            let mut mask = vec![false; row_count];
            let mut first_missing_column = None;
            let mut evaluated_any_branch = false;
            for group in groups {
                match evaluate_condition_group_with_options(group, dataset, dataset_presence_exists)
                {
                    Ok(branch) => {
                        evaluated_any_branch = true;
                        or_assign(&mut mask, &branch);
                    }
                    Err(source @ EngineError::MissingColumn(_)) => {
                        if first_missing_column.is_none() {
                            first_missing_column = Some(source);
                        }
                    }
                    Err(source) => return Err(source),
                }
                if mask.iter().all(|value| *value) {
                    break;
                }
            }
            if !evaluated_any_branch {
                if let Some(source) = first_missing_column {
                    return Err(source);
                }
            }
            Ok(mask)
        }
        ConditionGroup::Not(group) => {
            Ok(
                evaluate_condition_group_with_options(group, dataset, dataset_presence_exists)?
                    .into_iter()
                    .map(|value| !value)
                    .collect(),
            )
        }
        ConditionGroup::Leaf(condition) => {
            evaluate_condition_with_options(condition, dataset, dataset_presence_exists)
        }
    }
}

fn missing_column_is_false_in_all_group(group: &ConditionGroup) -> bool {
    matches!(
        group,
        ConditionGroup::Leaf(Condition {
            operator: Operator::MatchesRegex,
            ..
        })
    )
}

pub fn extract_target_variables(group: &ConditionGroup) -> Vec<String> {
    let mut variables = Vec::new();
    collect_target_variables(group, &mut variables);
    variables
}

fn issue_variables(rule: &ExecutableRule, dataset: &LoadedDataset) -> Vec<String> {
    let mut expanded = Vec::new();
    if contains_empty_within_except_last_row_operator(&rule.conditions) {
        for variable in extract_target_variables(&rule.conditions) {
            push_unique(
                &mut expanded,
                &expand_domain_placeholder(dataset, &variable),
            );
        }
    } else if contains_not_present_on_multiple_rows_within_operator(&rule.conditions) {
        collect_not_present_on_multiple_rows_variables(&rule.conditions, dataset, &mut expanded);
    } else if rule.output_variables.is_empty() {
        collect_issue_variables(&rule.conditions, dataset, &mut expanded);
    } else {
        for variable in &rule.output_variables {
            push_unique(&mut expanded, &expand_domain_placeholder(dataset, variable));
        }
    }
    expanded
}

fn collect_issue_variables(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
    variables: &mut Vec<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_issue_variables(group, dataset, variables);
            }
        }
        ConditionGroup::Not(group) => collect_issue_variables(group, dataset, variables),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = &condition.target {
                push_unique(variables, &expand_domain_placeholder(dataset, target));
            }
            collect_comparator_issue_variables(&condition.comparator, dataset, variables);
        }
    }
}

fn contains_empty_within_except_last_row_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(contains_empty_within_except_last_row_operator),
        ConditionGroup::Not(group) => contains_empty_within_except_last_row_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::EmptyWithinExceptLastRow)
        }
    }
}

fn contains_not_present_on_multiple_rows_within_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(contains_not_present_on_multiple_rows_within_operator),
        ConditionGroup::Not(group) => contains_not_present_on_multiple_rows_within_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::NotPresentOnMultipleRowsWithin)
        }
    }
}

fn collect_not_present_on_multiple_rows_variables(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
    variables: &mut Vec<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_not_present_on_multiple_rows_variables(group, dataset, variables);
            }
        }
        ConditionGroup::Not(group) => {
            collect_not_present_on_multiple_rows_variables(group, dataset, variables)
        }
        ConditionGroup::Leaf(condition) => {
            if matches!(condition.operator, Operator::NotPresentOnMultipleRowsWithin) {
                if let Some(within) = option_string(&condition.options.extra, "within") {
                    push_unique(variables, &expand_domain_placeholder(dataset, &within));
                }
                if let Some(target) = &condition.target {
                    push_unique(variables, &expand_domain_placeholder(dataset, target));
                }
            }
        }
    }
}

fn collect_comparator_issue_variables(
    value: &ValueExpr,
    dataset: &LoadedDataset,
    variables: &mut Vec<String>,
) {
    if let ValueExpr::ColumnRef(column) = value {
        let column = expand_domain_placeholder(dataset, column);
        if dataset.frame().column(&column).is_ok() {
            push_unique(variables, &column);
        }
    }
}

pub fn evaluate_condition(condition: &Condition, dataset: &LoadedDataset) -> Result<BooleanMask> {
    evaluate_condition_with_options(condition, dataset, false)
}

fn evaluate_condition_with_options(
    condition: &Condition,
    dataset: &LoadedDataset,
    dataset_presence_exists: bool,
) -> Result<BooleanMask> {
    let frame = dataset.frame();
    let row_count = frame.height();
    let operator = &condition.operator;
    let target = condition
        .target
        .as_deref()
        .ok_or(EngineError::MissingTarget)?;
    let target = expand_domain_placeholder(dataset, target);

    match operator {
        Operator::Exists => {
            let Some(column) = optional_column(frame, &target)? else {
                return Ok(vec![false; row_count]);
            };
            if dataset_presence_exists {
                return Ok(vec![true; row_count]);
            }
            return evaluate_column(column, row_count, |value, _row| Ok(!value.is_null()));
        }
        Operator::NotExists => {
            let Some(column) = optional_column(frame, &target)? else {
                return Ok(vec![true; row_count]);
            };
            if dataset_presence_exists {
                return Ok(vec![false; row_count]);
            }
            return evaluate_column(column, row_count, |value, _row| Ok(value.is_null()));
        }
        Operator::IsEmpty | Operator::IsNotEmpty => {
            let Some(column) = optional_column(frame, &target)? else {
                return Ok(vec![false; row_count]);
            };
            return evaluate_column(column, row_count, |value, _row| {
                let empty = ScalarValue::from_any_value(value).is_empty();
                Ok(matches!(operator, Operator::IsEmpty) == empty)
            });
        }
        _ => {}
    }

    let column = match optional_column(frame, &target)? {
        Some(column) => column,
        None if matches!(operator, Operator::IsNotUniqueSet | Operator::IsUniqueSet) => {
            return evaluate_unique_set(
                operator,
                dataset,
                frame,
                row_count,
                None,
                &condition.comparator,
                &condition.options,
            );
        }
        None if matches!(
            operator,
            Operator::IsNotContainedBy | Operator::IsNotContainedByCaseInsensitive
        ) =>
        {
            return Ok(vec![false; row_count]);
        }
        None => return Err(EngineError::MissingColumn(target)),
    };

    match operator {
        Operator::EqualTo
        | Operator::NotEqualTo
        | Operator::EqualToCaseInsensitive
        | Operator::NotEqualToCaseInsensitive => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            let type_insensitive =
                option_bool(&condition.options.extra, "type_insensitive").unwrap_or(false);
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let equal = scalar_matches_comparator(
                    &left,
                    comparator,
                    dataset,
                    frame,
                    row,
                    is_case_insensitive_operator(operator),
                    type_insensitive,
                )?;
                Ok(matches!(
                    operator,
                    Operator::EqualTo | Operator::EqualToCaseInsensitive
                ) == equal)
            })
        }
        Operator::Contains
        | Operator::DoesNotContain
        | Operator::ContainsCaseInsensitive
        | Operator::DoesNotContainCaseInsensitive => {
            let needle = string_comparator(operator, &condition.comparator)?;
            let needle = if is_case_insensitive_operator(operator) {
                needle.to_ascii_lowercase()
            } else {
                needle
            };
            evaluate_column(column, row_count, |value, _row| {
                let contains = ScalarValue::from_any_value(value)
                    .into_string()
                    .map(|haystack| {
                        string_contains_value(
                            &haystack,
                            &needle,
                            is_case_insensitive_operator(operator),
                        )
                    })
                    .unwrap_or(false);
                Ok(matches!(
                    operator,
                    Operator::Contains | Operator::ContainsCaseInsensitive
                ) == contains)
            })
        }
        Operator::IsContainedBy
        | Operator::IsNotContainedBy
        | Operator::IsContainedByCaseInsensitive
        | Operator::IsNotContainedByCaseInsensitive => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let contained = scalar_matches_comparator(
                    &left,
                    comparator,
                    dataset,
                    frame,
                    row,
                    is_case_insensitive_operator(operator),
                    false,
                )?;
                Ok(matches!(
                    operator,
                    Operator::IsContainedBy | Operator::IsContainedByCaseInsensitive
                ) == contained)
            })
        }
        Operator::ContainsAll | Operator::NotContainsAll => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_list_comparator(comparator, dataset, frame, row)?;
                let contains_all = scalar_contains_all(&left, &right, false);
                Ok(matches!(operator, Operator::ContainsAll) == contains_all)
            })
        }
        Operator::SharesNoElementsWith => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_list_comparator(comparator, dataset, frame, row)?;
                Ok(scalar_shares_no_elements_with(&left, &right))
            })
        }
        Operator::IsNotOrderedSubsetOf => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_list_comparator(comparator, dataset, frame, row)?;
                Ok(!scalar_is_ordered_subset_of(&left, &right))
            })
        }
        Operator::LessThan
        | Operator::LessThanOrEqualTo
        | Operator::GreaterThan
        | Operator::GreaterThanOrEqualTo => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_comparator(comparator, dataset, frame, row)?;
                Ok(compare_scalars(&left, &right)
                    .map(|ordering| match operator {
                        Operator::LessThan => ordering == Ordering::Less,
                        Operator::LessThanOrEqualTo => {
                            matches!(ordering, Ordering::Less | Ordering::Equal)
                        }
                        Operator::GreaterThan => ordering == Ordering::Greater,
                        Operator::GreaterThanOrEqualTo => {
                            matches!(ordering, Ordering::Greater | Ordering::Equal)
                        }
                        _ => false,
                    })
                    .unwrap_or(false))
            })
        }
        Operator::DateEqualTo
        | Operator::DateNotEqualTo
        | Operator::DateLessThan
        | Operator::DateLessThanOrEqualTo
        | Operator::DateGreaterThan
        | Operator::DateGreaterThanOrEqualTo => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_comparator(comparator, dataset, frame, row)?;
                Ok(compare_complete_dates(&left, &right)
                    .map(|ordering| match operator {
                        Operator::DateEqualTo => ordering == Ordering::Equal,
                        Operator::DateNotEqualTo => ordering != Ordering::Equal,
                        Operator::DateLessThan => ordering == Ordering::Less,
                        Operator::DateLessThanOrEqualTo => {
                            matches!(ordering, Ordering::Less | Ordering::Equal)
                        }
                        Operator::DateGreaterThan => ordering == Ordering::Greater,
                        Operator::DateGreaterThanOrEqualTo => {
                            matches!(ordering, Ordering::Greater | Ordering::Equal)
                        }
                        _ => false,
                    })
                    .unwrap_or(false))
            })
        }
        Operator::IsCompleteDate | Operator::IsIncompleteDate | Operator::InvalidDate => {
            evaluate_column(column, row_count, |value, _row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                let Some(state) = classify_date_value(&value) else {
                    return Ok(false);
                };
                Ok(match operator {
                    Operator::IsCompleteDate => matches!(state, DateValueState::Complete),
                    Operator::IsIncompleteDate => matches!(state, DateValueState::Incomplete),
                    Operator::InvalidDate => matches!(state, DateValueState::Invalid),
                    _ => false,
                })
            })
        }
        Operator::InvalidDuration => evaluate_column(column, row_count, |value, _row| {
            let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                return Ok(false);
            };
            let value = value.trim();
            Ok(!value.is_empty() && !is_valid_iso_duration(value))
        }),
        Operator::TargetIsNotSortedBy => evaluate_target_is_not_sorted_by(
            dataset,
            frame,
            row_count,
            column,
            &condition.comparator,
            &condition.options,
        ),
        Operator::EmptyWithinExceptLastRow => evaluate_empty_within_except_last_row(
            dataset,
            frame,
            row_count,
            column,
            &condition.comparator,
            &condition.options,
        ),
        Operator::DoesNotHaveNextCorrespondingRecord => {
            evaluate_does_not_have_next_corresponding_record(
                dataset,
                frame,
                row_count,
                column,
                &condition.comparator,
                &condition.options,
            )
        }
        Operator::NotPresentOnMultipleRowsWithin => evaluate_not_present_on_multiple_rows_within(
            dataset,
            frame,
            row_count,
            column,
            &condition.options,
        ),
        Operator::IsNotUniqueSet | Operator::IsUniqueSet => evaluate_unique_set(
            operator,
            dataset,
            frame,
            row_count,
            Some(column),
            &condition.comparator,
            &condition.options,
        ),
        Operator::IsNotUniqueRelationship => evaluate_not_unique_relationship(
            dataset,
            frame,
            row_count,
            column,
            &condition.comparator,
            &condition.options,
        ),
        Operator::IsInconsistentAcrossDataset => evaluate_inconsistent_across_dataset(
            dataset,
            frame,
            row_count,
            column,
            &condition.comparator,
        ),
        Operator::InconsistentEnumeratedColumns => {
            evaluate_inconsistent_enumerated_columns(frame, row_count, &target)
        }
        Operator::MatchesRegex | Operator::DoesNotMatchRegex => {
            let pattern = string_comparator(operator, &condition.comparator)?;
            let regex = Regex::new(&pattern)?;
            evaluate_column(column, row_count, |value, _row| {
                let matches = ScalarValue::from_any_value(value)
                    .into_string()
                    .map(|haystack| regex.is_match(&haystack))
                    .unwrap_or(false);
                Ok(matches!(operator, Operator::MatchesRegex) == matches)
            })
        }
        Operator::DoesNotMatchRegexFullString => {
            let pattern = string_comparator(operator, &condition.comparator)?;
            let regex = Regex::new(&format!("^(?:{pattern})$")).ok();
            if regex.is_none() && !is_usdm_ref_lookahead_pattern(&pattern) {
                Regex::new(&format!("^(?:{pattern})$"))?;
            }
            evaluate_column(column, row_count, |value, _row| {
                let Some(haystack) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if haystack.is_empty() {
                    return Ok(false);
                }
                let matches = if let Some(regex) = &regex {
                    regex.is_match(&haystack)
                } else {
                    usdm_ref_pattern_matches(&haystack)
                };
                Ok(!matches)
            })
        }
        Operator::DoesNotEqualStringPart => {
            let pattern = option_string(&condition.options.extra, "regex").ok_or_else(|| {
                EngineError::MissingComparator {
                    operator: operator.as_name().to_owned(),
                }
            })?;
            let regex = Regex::new(&pattern)?;
            evaluate_column(column, row_count, |value, row| {
                let Some(left) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                let right = resolve_scalar_comparator(&condition.comparator, dataset, frame, row)?
                    .into_string();
                let Some(right) = right else {
                    return Ok(false);
                };
                let Some(captures) = regex.captures(&right) else {
                    return Ok(false);
                };
                let Some(part) = captures.get(1).map(|part| part.as_str()) else {
                    return Ok(false);
                };
                Ok(left != part)
            })
        }
        Operator::LongerThan => {
            let max_len = length_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, _row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                Ok(value.chars().count() > max_len)
            })
        }
        Operator::StartsWith | Operator::PrefixNotEqualTo => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                let Some(prefix) =
                    resolve_scalar_comparator(comparator, dataset, frame, row)?.into_string()
                else {
                    return Ok(false);
                };
                let starts_with = value.starts_with(&prefix);
                Ok(matches!(operator, Operator::StartsWith) == starts_with)
            })
        }
        Operator::PrefixEqualTo => {
            let prefix_len = option_usize(&condition.options.extra, "prefix").ok_or_else(|| {
                EngineError::MissingComparator {
                    operator: operator.as_name().to_owned(),
                }
            })?;
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                let prefix = string_prefix(&value, prefix_len);
                let right = resolve_scalar_comparator(comparator, dataset, frame, row)?;
                Ok(scalar_equal_with_mode(
                    &ScalarValue::String(prefix),
                    &right,
                    false,
                    false,
                ))
            })
        }
        Operator::NotPrefixMatchesRegex => {
            let prefix_len = option_usize(&condition.options.extra, "prefix").ok_or_else(|| {
                EngineError::MissingComparator {
                    operator: operator.as_name().to_owned(),
                }
            })?;
            let pattern = string_comparator(operator, &condition.comparator)?;
            let regex = Regex::new(&pattern)?;
            evaluate_column(column, row_count, |value, _row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                Ok(!regex.is_match(&string_prefix(&value, prefix_len)))
            })
        }
        Operator::PrefixIsNotContainedBy | Operator::SuffixIsNotContainedBy => {
            let part_len = option_usize(
                &condition.options.extra,
                if matches!(operator, Operator::PrefixIsNotContainedBy) {
                    "prefix"
                } else {
                    "suffix"
                },
            )
            .ok_or_else(|| EngineError::MissingComparator {
                operator: operator.as_name().to_owned(),
            })?;
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                let part = if matches!(operator, Operator::PrefixIsNotContainedBy) {
                    string_prefix(&value, part_len)
                } else {
                    string_suffix(&value, part_len)
                };
                let contained = scalar_matches_comparator(
                    &ScalarValue::String(part),
                    comparator,
                    dataset,
                    frame,
                    row,
                    false,
                    false,
                )?;
                Ok(!contained)
            })
        }
        Operator::EndsWith => {
            let suffix = string_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, _row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                Ok(value.ends_with(&suffix))
            })
        }
        Operator::SuffixMatchesRegex | Operator::NotSuffixMatchesRegex => {
            let suffix_len = option_usize(&condition.options.extra, "suffix").ok_or_else(|| {
                EngineError::MissingComparator {
                    operator: operator.as_name().to_owned(),
                }
            })?;
            let pattern = string_comparator(operator, &condition.comparator)?;
            let regex = Regex::new(&pattern)?;
            evaluate_column(column, row_count, |value, _row| {
                let Some(value) = ScalarValue::from_any_value(value).into_string() else {
                    return Ok(false);
                };
                if value.is_empty() {
                    return Ok(false);
                }
                let suffix = value
                    .chars()
                    .rev()
                    .take(suffix_len)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>();
                let matches = regex.is_match(&suffix);
                Ok(matches!(operator, Operator::SuffixMatchesRegex) == matches)
            })
        }
        Operator::IsEmpty | Operator::IsNotEmpty => unreachable!("handled before target lookup"),
        Operator::Unsupported(name) => Err(EngineError::UnsupportedOperator(name.clone())),
        other => Err(EngineError::UnsupportedOperator(other.as_name().to_owned())),
    }
}

fn evaluate_column(
    column: &Column,
    row_count: usize,
    mut predicate: impl FnMut(AnyValue<'_>, usize) -> Result<bool>,
) -> Result<BooleanMask> {
    (0..row_count)
        .map(|row| predicate(column.get(row)?, row))
        .collect()
}

fn evaluate_target_is_not_sorted_by(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let sort_specs = sort_specs(comparator)?;
    let within = option_string(&options.extra, "within")
        .map(|value| expand_domain_placeholder(dataset, &value));
    let mut groups: std::collections::BTreeMap<String, Vec<SortRow>> =
        std::collections::BTreeMap::new();

    for row in 0..row_count {
        let group_key = match within.as_deref() {
            Some(column_name) => cell_string(frame, column_name, row)?.unwrap_or_default(),
            None => String::new(),
        };
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let sort_values = sort_specs
            .iter()
            .map(|spec| {
                let column_name = expand_domain_placeholder(dataset, &spec.column);
                let Some(column) = optional_column(frame, &column_name)? else {
                    return Ok(None);
                };
                Ok(Some(ScalarValue::from_any_value(column.get(row)?)))
            })
            .collect::<Result<Vec<_>>>()?;

        groups.entry(group_key).or_default().push(SortRow {
            row,
            target,
            sort_values,
        });
    }

    let mut mask = vec![false; row_count];
    for rows in groups.values() {
        let mut group_has_inversion = false;
        let group_has_uncertain_sort = rows
            .iter()
            .any(|row| row.sort_values.iter().any(is_uncertain_sort_value));

        for left_index in 0..rows.len() {
            for right_index in (left_index + 1)..rows.len() {
                let left = &rows[left_index];
                let right = &rows[right_index];
                let Some(sort_ordering) =
                    compare_sort_values(&left.sort_values, &right.sort_values, &sort_specs)
                else {
                    continue;
                };
                let Some(target_ordering) = compare_scalars(&left.target, &right.target) else {
                    continue;
                };
                if sort_ordering != Ordering::Equal
                    && target_ordering != Ordering::Equal
                    && sort_ordering != target_ordering
                {
                    mask[left.row] = true;
                    mask[right.row] = true;
                    group_has_inversion = true;
                }
            }
        }

        if group_has_inversion && group_has_uncertain_sort {
            for row in rows {
                if !matches!(row.target, ScalarValue::Null) {
                    mask[row.row] = true;
                }
            }
        }
    }

    Ok(mask)
}

fn evaluate_empty_within_except_last_row(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let group_column = expand_domain_placeholder(
        dataset,
        &column_name_comparator(&Operator::EmptyWithinExceptLastRow, comparator)?,
    );
    let ordering_column_name = option_string(&options.extra, "ordering").ok_or_else(|| {
        EngineError::MissingComparator {
            operator: Operator::EmptyWithinExceptLastRow.as_name().to_owned(),
        }
    })?;
    let ordering_column_name = expand_domain_placeholder(dataset, &ordering_column_name);
    let ordering_column = frame
        .column(&ordering_column_name)
        .map_err(|_| EngineError::MissingColumn(ordering_column_name.clone()))?;
    let sort_spec = SortSpec {
        column: ordering_column_name,
        descending: false,
        nulls_first: false,
    };

    let mut groups: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for row in 0..row_count {
        groups
            .entry(cell_string(frame, &group_column, row)?.unwrap_or_default())
            .or_default()
            .push(row);
    }

    let mut mask = vec![false; row_count];
    for rows in groups.values() {
        let mut sorted_rows = rows.clone();
        sorted_rows.sort_by(|left, right| {
            let left_value =
                ScalarValue::from_any_value(ordering_column.get(*left).unwrap_or(AnyValue::Null));
            let right_value =
                ScalarValue::from_any_value(ordering_column.get(*right).unwrap_or(AnyValue::Null));
            compare_optional_sort_value(Some(&left_value), Some(&right_value), &sort_spec)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.cmp(right))
        });
        let Some(last_row) = sorted_rows.last().copied() else {
            continue;
        };
        for row in rows.iter().copied().filter(|row| *row != last_row) {
            let value = ScalarValue::from_any_value(target_column.get(row)?);
            if value.is_empty() {
                mask[row] = true;
            }
        }
    }

    Ok(mask)
}

fn evaluate_does_not_have_next_corresponding_record(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let comparator_column_name = expand_domain_placeholder(
        dataset,
        &column_name_comparator(&Operator::DoesNotHaveNextCorrespondingRecord, comparator)?,
    );
    let comparator_column = frame
        .column(&comparator_column_name)
        .map_err(|_| EngineError::MissingColumn(comparator_column_name.clone()))?;
    let ordering_column_name = option_string(&options.extra, "ordering").ok_or_else(|| {
        EngineError::MissingComparator {
            operator: Operator::DoesNotHaveNextCorrespondingRecord
                .as_name()
                .to_owned(),
        }
    })?;
    let ordering_column_name = expand_domain_placeholder(dataset, &ordering_column_name);
    let ordering_column = frame
        .column(&ordering_column_name)
        .map_err(|_| EngineError::MissingColumn(ordering_column_name.clone()))?;
    let within = option_string(&options.extra, "within")
        .map(|value| expand_domain_placeholder(dataset, &value));
    let sort_spec = SortSpec {
        column: ordering_column_name,
        descending: false,
        nulls_first: false,
    };

    let ordering_values = (0..row_count)
        .map(|row| Ok(ScalarValue::from_any_value(ordering_column.get(row)?)))
        .collect::<Result<Vec<_>>>()?;
    let comparator_values = (0..row_count)
        .map(|row| Ok(ScalarValue::from_any_value(comparator_column.get(row)?)))
        .collect::<Result<Vec<_>>>()?;
    let target_values = (0..row_count)
        .map(|row| Ok(ScalarValue::from_any_value(target_column.get(row)?)))
        .collect::<Result<Vec<_>>>()?;

    let mut groups: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for row in 0..row_count {
        let group_key = match within.as_deref() {
            Some(column_name) => cell_string(frame, column_name, row)?.unwrap_or_default(),
            None => String::new(),
        };
        groups.entry(group_key).or_default().push(row);
    }

    let mut mask = vec![false; row_count];
    for rows in groups.values() {
        let mut sorted_rows = rows.clone();
        sorted_rows.sort_by(|left, right| {
            compare_optional_sort_value(
                Some(&ordering_values[*left]),
                Some(&ordering_values[*right]),
                &sort_spec,
            )
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.cmp(right))
        });
        for pair in sorted_rows.windows(2) {
            let current = pair[0];
            let next = pair[1];
            if !scalar_equal_with_mode(
                &target_values[current],
                &comparator_values[next],
                false,
                false,
            ) {
                mask[current] = true;
            }
        }
    }

    Ok(mask)
}

fn evaluate_not_present_on_multiple_rows_within(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let within = option_string(&options.extra, "within")
        .map(|value| expand_domain_placeholder(dataset, &value));
    let mut counts = std::collections::BTreeMap::<(String, String), usize>::new();
    let mut row_keys = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let group_key = match within.as_deref() {
            Some(column_name) => cell_string(frame, column_name, row)?.unwrap_or_default(),
            None => String::new(),
        };
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        if target.is_empty() {
            row_keys.push(None);
            continue;
        }
        let key = (group_key, target.to_string());
        *counts.entry(key.clone()).or_default() += 1;
        row_keys.push(Some(key));
    }

    Ok(row_keys
        .into_iter()
        .map(|key| key.is_some_and(|key| counts.get(&key).copied().unwrap_or_default() == 1))
        .collect())
}

#[derive(Debug)]
struct SortSpec {
    column: String,
    descending: bool,
    nulls_first: bool,
}

#[derive(Debug)]
struct SortRow {
    row: usize,
    target: ScalarValue,
    sort_values: Vec<Option<ScalarValue>>,
}

fn sort_specs(comparator: &ValueExpr) -> Result<Vec<SortSpec>> {
    let ValueExpr::List(values) = comparator else {
        return Err(EngineError::InvalidComparator {
            operator: Operator::TargetIsNotSortedBy.as_name().to_owned(),
            comparator: comparator.clone(),
        });
    };

    let specs = values
        .iter()
        .map(|value| {
            let Value::Object(object) = value else {
                return Err(EngineError::InvalidComparator {
                    operator: Operator::TargetIsNotSortedBy.as_name().to_owned(),
                    comparator: comparator.clone(),
                });
            };
            let Some(column) = object.get("name").and_then(Value::as_str) else {
                return Err(EngineError::InvalidComparator {
                    operator: Operator::TargetIsNotSortedBy.as_name().to_owned(),
                    comparator: comparator.clone(),
                });
            };
            let descending = object
                .get("sort_order")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("desc"));
            let nulls_first = object
                .get("null_position")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("first"));
            Ok(SortSpec {
                column: column.to_owned(),
                descending,
                nulls_first,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    if specs.is_empty() {
        return Err(EngineError::InvalidComparator {
            operator: Operator::TargetIsNotSortedBy.as_name().to_owned(),
            comparator: comparator.clone(),
        });
    }
    Ok(specs)
}

fn column_name_comparator(operator: &Operator, comparator: &ValueExpr) -> Result<String> {
    match comparator {
        ValueExpr::ColumnRef(value) => Ok(value.clone()),
        ValueExpr::Literal(Value::String(value)) => Ok(value.clone()),
        _ => Err(EngineError::InvalidComparator {
            operator: operator.as_name().to_owned(),
            comparator: comparator.clone(),
        }),
    }
}

fn compare_sort_values(
    left: &[Option<ScalarValue>],
    right: &[Option<ScalarValue>],
    specs: &[SortSpec],
) -> Option<Ordering> {
    for ((left, right), spec) in left.iter().zip(right).zip(specs) {
        let ordering = compare_optional_sort_value(left.as_ref(), right.as_ref(), spec)?;
        if ordering != Ordering::Equal {
            return Some(ordering);
        }
    }
    Some(Ordering::Equal)
}

fn compare_optional_sort_value(
    left: Option<&ScalarValue>,
    right: Option<&ScalarValue>,
    spec: &SortSpec,
) -> Option<Ordering> {
    let ordering = match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => {
            if spec.nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (Some(_), None) => {
            if spec.nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(left), Some(right)) => compare_scalars(left, right)?,
    };

    Some(if spec.descending {
        ordering.reverse()
    } else {
        ordering
    })
}

fn is_uncertain_sort_value(value: &Option<ScalarValue>) -> bool {
    let Some(ScalarValue::String(value)) = value else {
        return false;
    };
    let value = value.trim();
    !value.is_empty() && parse_complete_date(value).is_none() && is_incomplete_date(value)
}

fn record_level_issues(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    mask: &[bool],
    variables: &[String],
    message: &str,
) -> Result<Vec<ValidationIssue>> {
    mask.iter()
        .enumerate()
        .filter(|(_row, failed)| **failed)
        .map(|(row, _failed)| {
            Ok(ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: dataset.metadata().name.clone(),
                domain: dataset.metadata().domain.clone(),
                row: issue_row(dataset, row)?,
                variables: variables.to_vec(),
                message: message.to_owned(),
                usubjid: cell_string(dataset.frame(), "USUBJID", row)?,
                seq: sequence_value(dataset, row)?,
            })
        })
        .collect()
}

fn group_level_issues(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    mask: &[bool],
    variables: &[String],
    message: &str,
) -> Result<Vec<ValidationIssue>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut issues = Vec::new();
    for (row, _failed) in mask.iter().enumerate().filter(|(_row, failed)| **failed) {
        let signature = group_issue_signature(dataset, variables, row)?;
        if !seen.insert(signature) {
            continue;
        }
        issues.push(ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: dataset.metadata().name.clone(),
            domain: dataset.metadata().domain.clone(),
            row: issue_row(dataset, row)?,
            variables: variables.to_vec(),
            message: message.to_owned(),
            usubjid: cell_string(dataset.frame(), "USUBJID", row)?,
            seq: sequence_value(dataset, row)?,
        });
    }
    Ok(issues)
}

fn issue_row(dataset: &LoadedDataset, row: usize) -> Result<Option<usize>> {
    if let Some(value) = cell_string(dataset.frame(), SOURCE_ROW_COLUMN, row)?
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
    {
        return Ok(Some(value));
    }
    Ok(Some(row + 1))
}

fn group_issue_signature(
    dataset: &LoadedDataset,
    variables: &[String],
    row: usize,
) -> Result<Vec<String>> {
    variables
        .iter()
        .map(|variable| {
            cell_string(dataset.frame(), variable, row).map(|value| {
                format!(
                    "{}={}",
                    variable.to_ascii_uppercase(),
                    value.unwrap_or_default()
                )
            })
        })
        .collect()
}

fn dataset_level_issues(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    mask: &[bool],
    variables: &[String],
    message: &str,
) -> Vec<ValidationIssue> {
    if !mask.iter().any(|failed| *failed) {
        return Vec::new();
    }

    vec![ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        row: None,
        variables: variables.to_vec(),
        message: message.to_owned(),
        usubjid: None,
        seq: None,
    }]
}

fn outcome_message(actions: &[ActionSpec]) -> Option<String> {
    actions
        .iter()
        .find(|action| action.name == "generate_dataset_error_objects")
        .or_else(|| actions.first())
        .and_then(|action| action.params.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn contains_relationship_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_relationship_operator)
        }
        ConditionGroup::Not(group) => contains_relationship_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::IsNotUniqueRelationship)
        }
    }
}

fn contains_unique_set_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_unique_set_operator)
        }
        ConditionGroup::Not(group) => contains_unique_set_operator(group),
        ConditionGroup::Leaf(condition) => matches!(
            condition.operator,
            Operator::IsNotUniqueSet | Operator::IsUniqueSet
        ),
    }
}

fn collect_target_variables(group: &ConditionGroup, variables: &mut Vec<String>) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_target_variables(group, variables);
            }
        }
        ConditionGroup::Not(group) => collect_target_variables(group, variables),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = &condition.target {
                push_unique(variables, target);
            }
            collect_value_expr_variables(&condition.comparator, variables);
        }
    }
}

fn collect_value_expr_variables(value: &ValueExpr, variables: &mut Vec<String>) {
    if let ValueExpr::ColumnRef(column) = value {
        push_unique(variables, column);
    }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn is_usdm_ref_lookahead_pattern(pattern: &str) -> bool {
    pattern.contains("<usdm:ref")
        && pattern.contains("(?=")
        && pattern.contains("klass=")
        && pattern.contains("id=")
        && pattern.contains("attribute=")
        && (pattern.contains("</usdm:ref>") || pattern.contains("<\\/usdm:ref>"))
}

fn usdm_ref_pattern_matches(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("<usdm:ref") else {
        return false;
    };
    let Some(attributes) = remainder
        .strip_suffix("/>")
        .or_else(|| remainder.strip_suffix("></usdm:ref>"))
    else {
        return false;
    };

    let Some(klass) = quoted_attribute(attributes, "klass") else {
        return false;
    };
    let Some(id) = quoted_attribute(attributes, "id") else {
        return false;
    };
    let Some(attribute) = quoted_attribute(attributes, "attribute") else {
        return false;
    };

    !id.is_empty()
        && !klass.is_empty()
        && klass.chars().all(|value| value.is_ascii_alphabetic())
        && !attribute.is_empty()
        && attribute.chars().all(|value| value.is_ascii_alphabetic())
}

fn quoted_attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let bytes = attributes.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' {
            index += 1;
        }
        let key = &attributes[key_start..index];

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            continue;
        }
        index += 1;

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'"' {
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            continue;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != b'"' {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let value = &attributes[value_start..index];
        index += 1;

        if key == name {
            return Some(value);
        }
    }

    None
}

fn sequence_value(dataset: &LoadedDataset, row: usize) -> Result<Option<String>> {
    let candidates = sequence_columns(dataset);
    for column in candidates {
        if let Some(value) = cell_string(dataset.frame(), &column, row)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn sequence_columns(dataset: &LoadedDataset) -> Vec<String> {
    let mut columns = Vec::new();
    if let Some(domain) = &dataset.metadata().domain {
        columns.push(format!("{}SEQ", domain.to_ascii_uppercase()));
    }

    for column in dataset.frame().get_column_names() {
        let column = column.as_str();
        if column.eq_ignore_ascii_case("SEQ") || column.to_ascii_uppercase().ends_with("SEQ") {
            push_unique(&mut columns, column);
        }
    }

    columns
}

fn cell_string(frame: &DataFrame, column_name: &str, row: usize) -> Result<Option<String>> {
    let Some(column) = optional_column(frame, column_name)? else {
        return Ok(None);
    };
    Ok(ScalarValue::from_any_value(column.get(row)?).into_string())
}

fn optional_column<'a>(frame: &'a DataFrame, name: &str) -> Result<Option<&'a Column>> {
    match frame.column(name) {
        Ok(column) => Ok(Some(column)),
        Err(PolarsError::ColumnNotFound(_)) => {
            let Some(actual_name) = frame
                .get_column_names()
                .into_iter()
                .find(|column| column.as_str().eq_ignore_ascii_case(name))
            else {
                return Ok(None);
            };
            frame
                .column(actual_name.as_str())
                .map(Some)
                .map_err(Into::into)
        }
        Err(source) => Err(source.into()),
    }
}

fn required_comparator<'a>(
    operator: &Operator,
    comparator: &'a ValueExpr,
) -> Result<&'a ValueExpr> {
    if matches!(comparator, ValueExpr::Null) {
        match operator {
            Operator::EqualTo
            | Operator::NotEqualTo
            | Operator::EqualToCaseInsensitive
            | Operator::NotEqualToCaseInsensitive => Ok(comparator),
            _ => Err(EngineError::MissingComparator {
                operator: operator.as_name().to_owned(),
            }),
        }
    } else {
        Ok(comparator)
    }
}

fn string_comparator(operator: &Operator, comparator: &ValueExpr) -> Result<String> {
    match comparator {
        ValueExpr::Literal(Value::String(value)) => Ok(value.clone()),
        ValueExpr::Literal(value) if !value.is_null() => {
            Ok(json_value_to_scalar(value).to_string())
        }
        _ => Err(EngineError::InvalidComparator {
            operator: operator.as_name().to_owned(),
            comparator: comparator.clone(),
        }),
    }
}

fn length_comparator(operator: &Operator, comparator: &ValueExpr) -> Result<usize> {
    let value = match comparator {
        ValueExpr::Literal(Value::Number(value)) => value
            .as_u64()
            .or_else(|| {
                value.as_f64().and_then(|value| {
                    (value.is_finite() && value >= 0.0 && value.fract() == 0.0)
                        .then_some(value as u64)
                })
            })
            .and_then(|value| usize::try_from(value).ok()),
        ValueExpr::Literal(Value::String(value)) => value.trim().parse::<usize>().ok(),
        _ => None,
    };
    value.ok_or_else(|| EngineError::InvalidComparator {
        operator: operator.as_name().to_owned(),
        comparator: comparator.clone(),
    })
}

fn column_name_comparators(operator: &Operator, comparator: &ValueExpr) -> Result<Vec<String>> {
    match comparator {
        ValueExpr::Literal(Value::String(value)) => Ok(vec![value.clone()]),
        ValueExpr::ColumnRef(value) => Ok(vec![value.clone()]),
        ValueExpr::List(values) => {
            let columns = values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if columns.len() == values.len() && !columns.is_empty() {
                Ok(columns)
            } else {
                Err(EngineError::InvalidComparator {
                    operator: operator.as_name().to_owned(),
                    comparator: comparator.clone(),
                })
            }
        }
        _ => Err(EngineError::InvalidComparator {
            operator: operator.as_name().to_owned(),
            comparator: comparator.clone(),
        }),
    }
}

fn option_usize(map: &std::collections::BTreeMap<String, Value>, key: &str) -> Option<usize> {
    let value = map.get(key).or_else(|| {
        map.iter()
            .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
            .map(|(_key, value)| value)
    })?;
    match value {
        Value::Number(value) => value.as_u64().and_then(|value| usize::try_from(value).ok()),
        Value::String(value) => value.trim().parse::<usize>().ok(),
        _ => None,
    }
}

fn option_bool(map: &std::collections::BTreeMap<String, Value>, key: &str) -> Option<bool> {
    let value = map.get(key).or_else(|| {
        map.iter()
            .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
            .map(|(_key, value)| value)
    })?;
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => value.as_u64().and_then(|value| match value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" => Some(true),
            "false" | "f" | "no" | "n" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn option_string(map: &std::collections::BTreeMap<String, Value>, key: &str) -> Option<String> {
    let value = map.get(key).or_else(|| {
        map.iter()
            .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
            .map(|(_key, value)| value)
    })?;
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn is_case_insensitive_operator(operator: &Operator) -> bool {
    matches!(
        operator,
        Operator::EqualToCaseInsensitive
            | Operator::NotEqualToCaseInsensitive
            | Operator::ContainsCaseInsensitive
            | Operator::DoesNotContainCaseInsensitive
            | Operator::IsContainedByCaseInsensitive
            | Operator::IsNotContainedByCaseInsensitive
    )
}

fn compare_scalars(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return None;
    }

    match (left.as_number(), right.as_number()) {
        (Some(left), Some(right)) => left.partial_cmp(&right),
        _ => match (left.as_string(), right.as_string()) {
            (Some(left), Some(right)) => Some(left.cmp(right)),
            _ => None,
        },
    }
}

fn and_assign(mask: &mut [bool], other: &[bool]) {
    for (left, right) in mask.iter_mut().zip(other) {
        *left = *left && *right;
    }
}

fn or_assign(mask: &mut [bool], other: &[bool]) {
    for (left, right) in mask.iter_mut().zip(other) {
        *left = *left || *right;
    }
}

#[cfg(test)]
mod tests;
