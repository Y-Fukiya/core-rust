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

fn evaluate_unique_set(
    operator: &Operator,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: Option<&Column>,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let group_columns = expand_unique_set_group_columns(operator, comparator, dataset, frame)?;
    let group_column_values = group_columns
        .iter()
        .map(|column| optional_column(frame, column))
        .collect::<Result<Vec<_>>>()?;
    let regex = option_string(&options.extra, "regex")
        .map(|pattern| Regex::new(&pattern))
        .transpose()?;

    let mut counts = std::collections::BTreeMap::<Vec<String>, usize>::new();
    let mut row_keys = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = match target_column {
            Some(column) => ScalarValue::from_any_value(column.get(row)?).to_string(),
            None => "Not in dataset".to_owned(),
        };

        let mut key = Vec::with_capacity(group_columns.len() + 1);
        for column in &group_column_values {
            let value = match column {
                Some(column) => ScalarValue::from_any_value(column.get(row)?)
                    .into_string()
                    .unwrap_or_default(),
                None => "Not in dataset".to_owned(),
            };
            key.push(normalize_unique_set_key_value(&value, regex.as_ref()));
        }
        key.push(normalize_unique_set_key_value(&target, regex.as_ref()));
        *counts.entry(key.clone()).or_default() += 1;
        row_keys.push(Some(key));
    }

    Ok(row_keys
        .into_iter()
        .map(|key| {
            let duplicate =
                key.is_some_and(|key| counts.get(&key).copied().unwrap_or_default() > 1);
            matches!(operator, Operator::IsNotUniqueSet) == duplicate
        })
        .collect())
}

fn normalize_unique_set_key_value(value: &str, regex: Option<&Regex>) -> String {
    regex
        .and_then(|regex| regex.find(value))
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| value.to_owned())
}

fn expand_unique_set_group_columns(
    operator: &Operator,
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
) -> Result<Vec<String>> {
    let mut expanded = Vec::new();
    for column in column_name_comparators(operator, comparator)? {
        let column = expand_domain_placeholder(dataset, &column);
        if let Some(dynamic_columns) = dynamic_group_columns(frame, &column)? {
            expanded.extend(
                dynamic_columns
                    .into_iter()
                    .map(|column| expand_domain_placeholder(dataset, &column)),
            );
        } else {
            expanded.push(column);
        }
    }
    Ok(expanded)
}

fn dynamic_group_columns(frame: &DataFrame, column_name: &str) -> Result<Option<Vec<String>>> {
    let Some(column) = optional_column(frame, column_name)? else {
        return Ok(None);
    };
    for row in 0..frame.height() {
        let Some(value) = ScalarValue::from_any_value(column.get(row)?).into_string() else {
            continue;
        };
        if let Some(columns) = parse_group_column_list_literal(&value).filter(|columns| {
            !columns.is_empty()
                && columns.iter().all(|column| {
                    optional_column(frame, column).is_ok_and(|column| column.is_some())
                })
        }) {
            return Ok(Some(columns));
        }
    }
    Ok(None)
}

fn parse_group_column_list_literal(value: &str) -> Option<Vec<String>> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .filter_map(|part| {
                let column = part.trim().trim_matches('"').trim_matches('\'').trim();
                (!column.is_empty()).then(|| column.to_owned())
            })
            .collect(),
    )
}

fn evaluate_not_unique_relationship(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let related_column_name = expand_domain_placeholder(
        dataset,
        &column_name_comparator(&Operator::IsNotUniqueRelationship, comparator)?,
    );
    let related_column = frame
        .column(&related_column_name)
        .map_err(|_| EngineError::MissingColumn(related_column_name.clone()))?;

    let mut related_by_target =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut target_by_related =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut row_values = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let related = ScalarValue::from_any_value(related_column.get(row)?);

        let target = relationship_key(&target);
        let related = relationship_key(&related);
        related_by_target
            .entry(target.clone())
            .or_default()
            .insert(related.clone());
        target_by_related
            .entry(related.clone())
            .or_default()
            .insert(target.clone());
        row_values.push(Some((target, related)));
    }

    let direction = option_string(&options.extra, "direction");
    let target_to_comparator_only = direction.as_deref() == Some("target_to_comparator");
    let comparator_to_target_only = direction.as_deref() == Some("comparator_to_target");

    Ok(row_values
        .into_iter()
        .map(|values| {
            let Some((target, related)) = values else {
                return false;
            };
            (!comparator_to_target_only
                && related_by_target
                    .get(&target)
                    .is_some_and(|values| values.len() > 1))
                || (!target_to_comparator_only
                    && target_by_related
                        .get(&related)
                        .is_some_and(|values| values.len() > 1))
        })
        .collect())
}

fn relationship_key(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Null => String::new(),
        ScalarValue::String(value) if value.is_empty() => String::new(),
        value => value.to_string(),
    }
}

fn evaluate_inconsistent_across_dataset(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
) -> Result<BooleanMask> {
    let group_columns =
        column_name_comparators(&Operator::IsInconsistentAcrossDataset, comparator)?
            .into_iter()
            .map(|column| expand_domain_placeholder(dataset, &column))
            .collect::<Vec<_>>();
    let group_column_values = group_columns
        .iter()
        .map(|column| optional_column(frame, column))
        .collect::<Result<Vec<_>>>()?;

    let mut target_values_by_key =
        std::collections::BTreeMap::<Vec<String>, std::collections::BTreeSet<String>>::new();
    let mut row_keys = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let mut key = Vec::with_capacity(group_columns.len());
        for column in &group_column_values {
            let value = match column {
                Some(column) => ScalarValue::from_any_value(column.get(row)?)
                    .into_string()
                    .unwrap_or_default(),
                None => "Not in dataset".to_owned(),
            };
            key.push(value);
        }
        target_values_by_key
            .entry(key.clone())
            .or_default()
            .insert(target.to_string());
        row_keys.push(Some(key));
    }

    Ok(row_keys
        .into_iter()
        .map(|key| {
            key.is_some_and(|key| {
                target_values_by_key
                    .get(&key)
                    .map(|values| values.len() > 1)
                    .unwrap_or(false)
            })
        })
        .collect())
}

fn evaluate_inconsistent_enumerated_columns(
    frame: &DataFrame,
    row_count: usize,
    target: &str,
) -> Result<BooleanMask> {
    let columns = enumerated_columns(frame, target)?;
    (0..row_count)
        .map(|row| {
            let mut saw_empty = false;
            for column in &columns {
                let value = ScalarValue::from_any_value(column.get(row)?);
                if value.is_empty() {
                    saw_empty = true;
                } else if saw_empty {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .collect()
}

fn enumerated_columns<'a>(frame: &'a DataFrame, target: &str) -> Result<Vec<&'a Column>> {
    let mut columns = Vec::new();
    columns.push(
        frame
            .column(target)
            .map_err(|_| EngineError::MissingColumn(target.to_owned()))?,
    );
    for index in 1.. {
        let name = format!("{target}{index}");
        let Some(column) = optional_column(frame, &name)? else {
            break;
        };
        columns.push(column);
    }
    Ok(columns)
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

fn scalar_matches_comparator(
    left: &ScalarValue,
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
    case_insensitive: bool,
    type_insensitive: bool,
) -> Result<bool> {
    match comparator {
        ValueExpr::List(values) => Ok(values.iter().map(json_value_to_scalar).any(|right| {
            scalar_contained_by_value(left, &right, case_insensitive, type_insensitive)
        })),
        _ => {
            let right = resolve_scalar_comparator(comparator, dataset, frame, row)?;
            Ok(scalar_contained_by_value(
                left,
                &right,
                case_insensitive,
                type_insensitive,
            ))
        }
    }
}

fn string_prefix(value: &str, len: usize) -> String {
    value.chars().take(len).collect()
}

fn string_suffix(value: &str, len: usize) -> String {
    value
        .chars()
        .rev()
        .take(len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn resolve_scalar_comparator(
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
) -> Result<ScalarValue> {
    match comparator {
        ValueExpr::Literal(value) => Ok(json_value_to_scalar(value)),
        ValueExpr::Null => Ok(ScalarValue::Null),
        ValueExpr::ColumnRef(column_name) => {
            let column_name = expand_domain_placeholder(dataset, column_name);
            let Some(column) = optional_column(frame, &column_name)? else {
                return Ok(ScalarValue::String(column_name));
            };
            Ok(ScalarValue::from_any_value(column.get(row)?))
        }
        ValueExpr::List(_) => Err(EngineError::InvalidComparator {
            operator: "scalar_comparison".to_owned(),
            comparator: comparator.clone(),
        }),
    }
}

fn resolve_scalar_list_comparator(
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
) -> Result<ScalarValue> {
    let ValueExpr::List(values) = comparator else {
        return resolve_scalar_comparator(comparator, dataset, frame, row);
    };

    let mut resolved = Vec::new();
    for value in values {
        if let Some(reference) = value.as_str() {
            if let Some(column) = reference_column(frame, dataset, reference)? {
                let scalar = ScalarValue::from_any_value(column.get(row)?);
                resolved.extend(scalar_list_values(&scalar).map(|value| value.to_string()));
                continue;
            }
        }
        let scalar = json_value_to_scalar(value);
        resolved.extend(scalar_list_values(&scalar).map(|value| value.to_string()));
    }

    Ok(ScalarValue::String(resolved.join("|")))
}

fn reference_column<'a>(
    frame: &'a DataFrame,
    dataset: &LoadedDataset,
    value: &str,
) -> Result<Option<&'a Column>> {
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let mut candidates = vec![raw.to_owned()];
    if let Some(clean) = raw
        .strip_prefix('$')
        .filter(|reference| !reference.is_empty())
    {
        candidates.push(clean.to_owned());
    }

    for candidate in candidates {
        let column_name = expand_domain_placeholder(dataset, &candidate);
        if let Some(column) = optional_column(frame, &column_name)? {
            return Ok(Some(column));
        }
    }

    Ok(None)
}

fn expand_domain_placeholder(dataset: &LoadedDataset, name: &str) -> String {
    let Some(suffix) = name.strip_prefix("--") else {
        return name.to_owned();
    };
    let Some(prefix) = domain_prefix(dataset) else {
        return name.to_owned();
    };
    format!("{}{}", prefix, suffix.to_ascii_uppercase())
}

fn domain_prefix(dataset: &LoadedDataset) -> Option<String> {
    dataset
        .metadata()
        .domain
        .as_deref()
        .filter(|domain| !domain.trim().is_empty())
        .or_else(|| {
            (!dataset.metadata().name.trim().is_empty()).then_some(dataset.metadata().name.as_str())
        })
        .map(|domain| domain.trim().to_ascii_uppercase())
}

fn json_value_to_scalar(value: &Value) -> ScalarValue {
    match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(value) => ScalarValue::Bool(*value),
        Value::Number(value) => value
            .as_f64()
            .map(ScalarValue::Number)
            .unwrap_or_else(|| ScalarValue::String(value.to_string())),
        Value::String(value) => ScalarValue::String(value.clone()),
        other => ScalarValue::String(other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ScalarValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl ScalarValue {
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
            return Self::Number(value);
        }

        Self::String(value.to_string())
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::String(value) => value.is_empty(),
            _ => false,
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    fn as_type_insensitive_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) if value.is_finite() => Some(*value),
            Self::String(value) => value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite()),
            _ => None,
        }
    }

    fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn into_string(self) -> Option<String> {
        match self {
            Self::Null => None,
            Self::Bool(value) => Some(value.to_string()),
            Self::Number(value) => Some(value.to_string()),
            Self::String(value) => Some(value),
        }
    }
}

impl std::fmt::Display for ScalarValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
            Self::String(value) => f.write_str(value),
        }
    }
}

fn scalar_equal_with_mode(
    left: &ScalarValue,
    right: &ScalarValue,
    case_insensitive: bool,
    type_insensitive: bool,
) -> bool {
    if type_insensitive {
        if let (Some(left), Some(right)) = (
            left.as_type_insensitive_number(),
            right.as_type_insensitive_number(),
        ) {
            return left == right;
        }
    }

    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => true,
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => left == right,
        (ScalarValue::Bool(left), ScalarValue::String(right))
        | (ScalarValue::String(right), ScalarValue::Bool(left)) => {
            match right.trim().to_ascii_lowercase().as_str() {
                "true" => *left,
                "false" => !*left,
                _ => false,
            }
        }
        (ScalarValue::String(left), ScalarValue::String(right)) if case_insensitive => {
            left.eq_ignore_ascii_case(right)
        }
        (ScalarValue::String(left), ScalarValue::String(right)) => left == right,
        (ScalarValue::Number(left), ScalarValue::Number(right)) => left == right,
        _ => false,
    }
}

fn scalar_contained_by_value(
    left: &ScalarValue,
    right: &ScalarValue,
    case_insensitive: bool,
    type_insensitive: bool,
) -> bool {
    if scalar_equal_with_mode(left, right, case_insensitive, type_insensitive) {
        return true;
    }

    let ScalarValue::String(right) = right else {
        return false;
    };
    if !right.contains('|') {
        return false;
    }

    right.split('|').any(|part| {
        let part = part.trim();
        scalar_equal_with_mode(
            left,
            &ScalarValue::String(part.to_owned()),
            case_insensitive,
            type_insensitive,
        ) || scalar_string_equal_with_mode(left, part, case_insensitive)
    })
}

fn scalar_contains_all(left: &ScalarValue, right: &ScalarValue, case_insensitive: bool) -> bool {
    scalar_list_values(right).all(|value| {
        scalar_contained_by_value(&value, left, case_insensitive, false)
            || scalar_string_equal_with_mode(left, &value.to_string(), case_insensitive)
    })
}

fn scalar_shares_no_elements_with(left: &ScalarValue, right: &ScalarValue) -> bool {
    !scalar_list_values(left).any(|left_value| {
        scalar_list_values(right)
            .any(|right_value| scalar_equal_with_mode(&left_value, &right_value, false, false))
    })
}

fn scalar_is_ordered_subset_of(left: &ScalarValue, right: &ScalarValue) -> bool {
    let left_values = scalar_list_values(left)
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let right_values = scalar_list_values(right)
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if left_values.is_empty() {
        return true;
    }

    let mut right_index = 0;
    for left_value in left_values {
        let Some(next_index) = right_values[right_index..]
            .iter()
            .position(|right_value| right_value == &left_value)
        else {
            return false;
        };
        right_index += next_index + 1;
    }
    true
}

fn scalar_list_values(value: &ScalarValue) -> Box<dyn Iterator<Item = ScalarValue> + '_> {
    match value {
        ScalarValue::String(value) if value.contains('|') => Box::new(
            value
                .split('|')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| ScalarValue::String(part.to_owned())),
        ),
        ScalarValue::String(value) if value.trim().is_empty() => Box::new(std::iter::empty()),
        other => Box::new(std::iter::once(other.clone())),
    }
}

fn string_contains_value(haystack: &str, needle: &str, case_insensitive: bool) -> bool {
    if haystack.contains('|') {
        return haystack
            .split('|')
            .map(str::trim)
            .any(|part| string_equal_with_mode(part, needle, case_insensitive));
    }

    if case_insensitive {
        haystack
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    } else {
        haystack.contains(needle)
    }
}

fn scalar_string_equal_with_mode(left: &ScalarValue, right: &str, case_insensitive: bool) -> bool {
    string_equal_with_mode(&left.to_string(), right, case_insensitive)
}

fn string_equal_with_mode(left: &str, right: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
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

fn compare_complete_dates(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    Some(
        parse_orderable_date_for_comparison(left.as_string()?)?
            .cmp(&parse_orderable_date_for_comparison(right.as_string()?)?),
    )
}

fn parse_orderable_date_for_comparison(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    parse_orderable_complete_date(value).or_else(|| parse_orderable_incomplete_date(value))
}

fn parse_orderable_incomplete_date(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    if value.len() == 4 && value.chars().all(|character| character.is_ascii_digit()) {
        return Some((parse_fixed_digits(value)?, 1, 1, 0, 0, 0));
    }

    if value.len() == 7 && value.as_bytes().get(4) == Some(&b'-') {
        let year = parse_fixed_digits(value.get(0..4)?)?;
        let month = parse_fixed_digits(value.get(5..7)?)? as u8;
        if (1..=12).contains(&month) {
            return Some((year, month, 1, 0, 0, 0));
        }
    }

    None
}

fn parse_orderable_complete_date(value: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    let (year, month, day) = parse_complete_date(value)?;
    let remainder = value.get(10..).unwrap_or_default();
    if remainder.is_empty() {
        return Some((year, month, day, 0, 0, 0));
    }
    let time = remainder
        .strip_prefix('T')
        .or_else(|| remainder.strip_prefix(' '))?;
    let (hour_text, after_hour) = time.split_once(':')?;
    if !(1..=2).contains(&hour_text.len())
        || !hour_text
            .chars()
            .all(|character| character.is_ascii_digit())
        || after_hour.len() < 2
    {
        return None;
    }
    let hour = parse_fixed_digits(hour_text)? as u8;
    let minute = parse_fixed_digits(after_hour.get(0..2)?)? as u8;
    let second = if after_hour.as_bytes().get(2) == Some(&b':') {
        parse_fixed_digits(after_hour.get(3..5)?)? as u8
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((year, month, day, hour, minute, second))
}

fn parse_complete_date(value: &str) -> Option<(u16, u8, u8)> {
    let date = value.get(..10)?;
    let remainder = value.get(10..).unwrap_or_default();
    if !remainder.is_empty() && !remainder.starts_with('T') {
        return None;
    }

    let bytes = date.as_bytes();
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }

    let year = parse_fixed_digits(date.get(0..4)?)?;
    let month = parse_fixed_digits(date.get(5..7)?)? as u8;
    let day = parse_fixed_digits(date.get(8..10)?)? as u8;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }

    Some((year, month, day))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateValueState {
    Complete,
    Incomplete,
    Invalid,
}

fn classify_date_value(value: &str) -> Option<DateValueState> {
    let value = value.trim();
    if value.is_empty() {
        return Some(DateValueState::Incomplete);
    }
    if parse_orderable_complete_date(value).is_some() {
        return Some(DateValueState::Complete);
    }
    if parse_complete_date(value).is_some()
        && value
            .get(10..)
            .and_then(|remainder| remainder.strip_prefix('T'))
            .is_some_and(is_incomplete_iso_datetime_time)
    {
        return Some(DateValueState::Incomplete);
    }
    if is_incomplete_date(value) {
        return Some(DateValueState::Incomplete);
    }
    Some(DateValueState::Invalid)
}

fn is_incomplete_date(value: &str) -> bool {
    if value.len() == 4 {
        return value.chars().all(|character| character.is_ascii_digit());
    }

    if value.len() == 7 {
        let year = value.get(0..4);
        let separator = value.get(4..5);
        let month = value.get(5..7);
        if matches!(separator, Some("-"))
            && year.is_some_and(|year| year.chars().all(|character| character.is_ascii_digit()))
            && month
                .and_then(parse_fixed_digits)
                .is_some_and(|month| (1..=12).contains(&month))
        {
            return true;
        }
    }

    if let Some(month_day) = value.strip_prefix("--") {
        if month_day.len() == 5
            && month_day.as_bytes().get(2) == Some(&b'-')
            && parse_fixed_digits(&month_day[0..2]).is_some_and(|month| (1..=12).contains(&month))
            && parse_fixed_digits(&month_day[3..5]).is_some_and(|day| (1..=31).contains(&day))
        {
            return true;
        }
    }

    if value.len() == 9 && value.as_bytes().get(4..7) == Some(&b"---"[..]) {
        return value[0..4]
            .chars()
            .all(|character| character.is_ascii_digit())
            && parse_fixed_digits(&value[7..9]).is_some_and(|day| (1..=31).contains(&day));
    }

    if value.len() == 10 && value.as_bytes().get(4..8) == Some(&b"----"[..]) {
        return value[0..4]
            .chars()
            .all(|character| character.is_ascii_digit())
            && parse_fixed_digits(&value[8..10]).is_some_and(|day| (1..=31).contains(&day));
    }

    if value
        .strip_prefix("-----T")
        .is_some_and(is_incomplete_iso_time)
    {
        return true;
    }

    false
}

fn is_incomplete_iso_time(value: &str) -> bool {
    if value.len() < 2 {
        return false;
    }
    parse_fixed_digits(&value[0..2]).is_some_and(|hour| hour <= 23)
}

fn is_incomplete_iso_datetime_time(value: &str) -> bool {
    if value.len() == 2 {
        return parse_fixed_digits(value).is_some_and(|hour| hour <= 23);
    }

    value.contains('-') && value.contains(':')
}

fn parse_fixed_digits(value: &str) -> Option<u16> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<u16>().ok())
        .flatten()
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn is_valid_iso_duration(value: &str) -> bool {
    let Some(mut rest) = value.strip_prefix('P') else {
        return false;
    };
    if rest.is_empty() || rest.contains('-') {
        return false;
    }

    if let Some(week) = rest.strip_suffix('W') {
        return is_valid_duration_number(week);
    }

    let mut in_time = false;
    let mut number = String::new();
    let mut saw_component = false;
    let mut last_date_order = 0;
    let mut last_time_order = 0;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if character == 'T' {
            if in_time || !number.is_empty() {
                return false;
            }
            in_time = true;
            continue;
        }
        if character.is_ascii_digit() || character == '.' || character == ',' {
            number.push(character);
            continue;
        }
        if !is_valid_duration_number(&number) {
            return false;
        }

        if in_time {
            let order = match character {
                'H' => 1,
                'M' => 2,
                'S' => 3,
                _ => return false,
            };
            if order <= last_time_order {
                return false;
            }
            last_time_order = order;
        } else {
            let order = match character {
                'Y' => 1,
                'M' => 2,
                'D' => 4,
                _ => return false,
            };
            if order <= last_date_order {
                return false;
            }
            last_date_order = order;
        }

        number.clear();
        saw_component = true;
    }

    saw_component && number.is_empty()
}

fn is_valid_duration_number(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let separator_count = value
        .chars()
        .filter(|character| *character == '.' || *character == ',')
        .count();
    separator_count <= 1
        && !value.starts_with(['.', ','])
        && !value.ends_with(['.', ','])
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.' || character == ',')
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
mod tests {
    use std::fs;

    use core_data::load_dataset_package_json;
    use core_rule_model::{
        ActionSpec, Condition, ConditionGroup, ExecutableRule, Operator, OperatorOptions, RuleType,
        Sensitivity, ValueExpr,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn test_dataset() -> LoadedDataset {
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
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3", "SUBJ4"],
        "AESEQ": [1, 2, 3, null],
        "AESEQ_COPY": [1, 20, 3, null],
        "DOMAIN": ["AE", "CM", "", null],
        "TERM": ["Headache", "nausea", "", null],
        "STARTDTC": ["2024-01-02", "2024-01-03T12:30:00", "2024-01", "2024-13-01"],
        "DUR": ["P1D", "PT2H", "P1Y2M", "P-1D"],
        "FLAG": [true, false, null, true]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset")
    }

    fn sort_dataset() -> LoadedDataset {
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
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "AESEQ": [1, 3, 2, 1, 2],
        "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03", "2024-01-01", "2024-01-02"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset")
    }

    fn end_date_dataset() -> LoadedDataset {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
        "SESEQ": [1, 2, 3, 1],
        "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03", "2024-02-01"],
        "SEENDTC": ["2024-01-02", "", "", ""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset")
    }

    fn relationship_dataset() -> LoadedDataset {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
        "VISITNUM": [1, 2, null, null],
        "RELID": ["R1", "R1", "R2", "R3"],
        "LEFT": ["A", "A", "C", "D"],
        "RIGHT": ["1", "2", "3", "3"],
        "LEFT_EMPTY": ["A", "A", "", "C"],
        "RIGHT_EMPTY": ["1", "", "1", "2"],
        "TARGET_EMPTY_DUP": ["", "", "X", "Y"],
        "GROUP_DUP": ["G", "G", "H", "I"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset")
    }

    fn enumerated_dataset() -> LoadedDataset {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "co.xpt",
      "domain": "CO",
      "records": {
        "COSEQ": [1, 2, 3],
        "COVAL": ["primary", "", "primary"],
        "COVAL1": ["", "", "secondary"],
        "COVAL2": ["", "later", ""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset")
    }

    fn condition(target: &str, operator: Operator, comparator: ValueExpr) -> Condition {
        Condition {
            target: Some(target.to_owned()),
            operator,
            comparator,
            options: OperatorOptions::default(),
        }
    }

    fn condition_with_options(
        target: &str,
        operator: Operator,
        comparator: ValueExpr,
        options: serde_json::Map<String, Value>,
    ) -> Condition {
        Condition {
            target: Some(target.to_owned()),
            operator,
            comparator,
            options: OperatorOptions {
                extra: options.into_iter().collect(),
            },
        }
    }

    fn literal(value: impl Into<Value>) -> ValueExpr {
        ValueExpr::Literal(value.into())
    }

    fn rule(
        sensitivity: Option<Sensitivity>,
        conditions: ConditionGroup,
        message: &str,
    ) -> ExecutableRule {
        ExecutableRule {
            core_id: "CORE-TEST-0001".to_owned(),
            author: None,
            sensitivity,
            executability: None,
            description: None,
            authorities: Vec::new(),
            standards: Vec::new(),
            classes: None,
            domains: None,
            datasets: None,
            entities: None,
            rule_type: RuleType::RecordData,
            conditions,
            actions: vec![ActionSpec {
                name: "generate_dataset_error_objects".to_owned(),
                params: json!({ "message": message }),
            }],
            operations: Vec::new(),
            output_variables: Vec::new(),
            grouping_variables: Vec::new(),
            use_case: None,
            status: None,
            raw: None,
        }
    }

    #[test]
    fn evaluates_exists_and_not_exists() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::Exists, ValueExpr::Null),
                &dataset
            )
            .expect("exists"),
            vec![true, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::NotExists, ValueExpr::Null),
                &dataset
            )
            .expect("not exists"),
            vec![false, false, false, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("MISSING", Operator::NotExists, ValueExpr::Null),
                &dataset
            )
            .expect("missing not exists"),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn evaluates_domain_prefixed_placeholder_columns() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("--SEQ", Operator::GreaterThan, literal(2)),
                &dataset
            )
            .expect("domain placeholder"),
            vec![false, false, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("--SEQ_COPY", Operator::Exists, ValueExpr::Null),
                &dataset
            )
            .expect("domain placeholder exists"),
            vec![true, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("--MISSING", Operator::NotExists, ValueExpr::Null),
                &dataset
            )
            .expect("domain placeholder missing"),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn evaluates_equal_and_not_equal() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::EqualTo, literal("AE")),
                &dataset
            )
            .expect("equal"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::NotEqualTo, literal("AE")),
                &dataset
            )
            .expect("not equal"),
            vec![false, true, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "AESEQ",
                    Operator::EqualTo,
                    ValueExpr::ColumnRef("AESEQ_COPY".to_owned())
                ),
                &dataset
            )
            .expect("column ref equal"),
            vec![true, false, true, true]
        );
    }

    #[test]
    fn missing_column_ref_comparator_falls_back_to_literal_string() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "DOMAIN",
                    Operator::EqualTo,
                    ValueExpr::ColumnRef("AE".to_owned())
                ),
                &dataset
            )
            .expect("missing column ref fallback"),
            vec![true, false, false, false]
        );
    }

    #[test]
    fn condition_targets_match_columns_case_insensitively() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "domain",
                    Operator::EqualTo,
                    ValueExpr::ColumnRef("AE".to_owned())
                ),
                &dataset
            )
            .expect("case-insensitive target"),
            vec![true, false, false, false]
        );
    }

    #[test]
    fn validation_issues_ignore_missing_column_ref_literal_fallback_variables() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition(
                "DOMAIN",
                Operator::NotEqualTo,
                ValueExpr::ColumnRef("AE".to_owned()),
            )),
            "DOMAIN must be AE",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.errors[0].variables, vec!["DOMAIN"]);
    }

    #[test]
    fn equality_respects_string_and_numeric_types() {
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
        "CODE": ["01", "1", "1.0"],
        "AVAL": [1, 2, 10]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition("CODE", Operator::EqualTo, literal("1")),
                &dataset
            )
            .expect("string equal"),
            vec![false, true, false]
        );
        assert_eq!(
            evaluate_condition(&condition("CODE", Operator::EqualTo, literal(1)), &dataset)
                .expect("string not coerced to number"),
            vec![false, false, false]
        );
        assert_eq!(
            evaluate_condition(&condition("AVAL", Operator::EqualTo, literal(1)), &dataset)
                .expect("numeric equal"),
            vec![true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AVAL", Operator::EqualTo, literal("1")),
                &dataset
            )
            .expect("number not coerced to string"),
            vec![false, false, false]
        );
        assert!(!scalar_equal_with_mode(
            &ScalarValue::Bool(true),
            &ScalarValue::Number(1.0),
            false,
            false
        ));
        assert!(scalar_equal_with_mode(
            &ScalarValue::Bool(true),
            &ScalarValue::String("True".to_owned()),
            false,
            false
        ));
        assert!(scalar_equal_with_mode(
            &ScalarValue::Bool(false),
            &ScalarValue::String("false".to_owned()),
            false,
            false
        ));
    }

    #[test]
    fn evaluates_case_insensitive_equality_and_list_comparators() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::EqualToCaseInsensitive,
                    literal("headache")
                ),
                &dataset
            )
            .expect("case-insensitive equal"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::NotEqualToCaseInsensitive,
                    literal("HEADACHE")
                ),
                &dataset
            )
            .expect("case-insensitive not equal"),
            vec![false, true, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "DOMAIN",
                    Operator::EqualTo,
                    ValueExpr::List(vec![json!("AE"), json!("VS")])
                ),
                &dataset
            )
            .expect("equal list comparator"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::EqualToCaseInsensitive,
                    ValueExpr::List(vec![json!("HEADACHE"), json!("DIZZINESS")])
                ),
                &dataset
            )
            .expect("case-insensitive equal list comparator"),
            vec![true, false, false, false]
        );
    }

    #[test]
    fn evaluates_contains_and_regex() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::Contains, literal("ache")),
                &dataset
            )
            .expect("contains"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::DoesNotContain, literal("ache")),
                &dataset
            )
            .expect("does not contain"),
            vec![false, true, true, true]
        );
        assert!(string_contains_value("Headache", "ache", false));
        assert!(string_contains_value("ARMCD|SPECIES", "ARMCD", false));
        assert!(!string_contains_value("ARMCDxxx|SPECIES", "ARMCD", false));
        assert!(string_contains_value("armcd|species", "ARMCD", true));
        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::ContainsCaseInsensitive, literal("ACHE")),
                &dataset
            )
            .expect("case-insensitive contains"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::DoesNotContainCaseInsensitive,
                    literal("ACHE")
                ),
                &dataset
            )
            .expect("case-insensitive does not contain"),
            vec![false, true, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::MatchesRegex, literal("^[A-Z][a-z]+$")),
                &dataset
            )
            .expect("matches regex"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::DoesNotMatchRegex,
                    literal("^[A-Z][a-z]+$")
                ),
                &dataset
            )
            .expect("does not match regex"),
            vec![false, true, true, true]
        );
    }

    #[test]
    fn evaluates_open_rules_not_matches_regex_as_full_non_empty_string() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::DoesNotMatchRegexFullString,
                    literal("[a-z]+$")
                ),
                &dataset
            )
            .expect("open rules not_matches_regex"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::DoesNotMatchRegexFullString,
                    literal(r#"^-?(\d+(\.\d+)?$)|(\.\d+$)"#)
                ),
                &dataset
            )
            .expect("open rules numeric not_matches_regex"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn evaluates_usdm_ref_lookahead_regex_fallback() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ParameterMap.csv",
      "domain": "ParameterMap",
      "records": {
        "reference": [
          "<usdm:ref klass=\"Activity\" id=\"Activity_1\" attribute=\"label\"></usdm:ref>",
          "<usdm:ref attribute=\"label\" id=\"Activity_1\" klass=\"Activity\"/>",
          "<usdm:ref attribute=\"maxValue\" id=\"Range 1\" klass=\"Range\"/>",
          "<usdm:ref klass=\"Range1\" id=\"Range_3\" attribute=\"maxValue\"></usdm:ref>",
          "<usdm:ref id=\"Activity_6\" attribute=\"label\" class=\"Activity\"></usdm:ref>",
          "<usdm:ref attribute=\"label\" klass=\"Activity\" id=\"Activity_9\"></usdm:ref>  ",
          " <usdm:ref attribute=\"label\" klass=\"Activity\" id=\"Activity_9\"></usdm:ref>",
          "a piece of text that includes usdm:ref"
        ]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");
        let pattern = r#"^<usdm:ref((?=[^>]* klass=\"[a-zA-Z]+\")(?=[^>]* id=\"([^\"]+)\")(?=[^>]* attribute=\"[a-zA-Z]+\")[^>]*)(\/>|><\/usdm:ref>)$"#;

        assert_eq!(
            evaluate_condition(
                &condition(
                    "reference",
                    Operator::DoesNotMatchRegexFullString,
                    literal(pattern)
                ),
                &dataset
            )
            .expect("USDM ref lookahead fallback"),
            vec![false, false, false, true, true, true, true, true]
        );
    }

    #[test]
    fn evaluates_longer_than_against_character_count() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::LongerThan, literal(6)),
                &dataset
            )
            .expect("longer than"),
            vec![true, false, false, false]
        );
    }

    #[test]
    fn evaluates_prefix_and_suffix_regex_operators() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::StartsWith, literal("Head")),
                &dataset
            )
            .expect("starts with"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::EndsWith, literal("ache")),
                &dataset
            )
            .expect("ends with"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "TERM",
                    Operator::SuffixMatchesRegex,
                    literal("ache"),
                    serde_json::Map::from_iter([("suffix".to_owned(), json!(4))])
                ),
                &dataset
            )
            .expect("suffix matches regex"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "TERM",
                    Operator::NotSuffixMatchesRegex,
                    literal("ache"),
                    serde_json::Map::from_iter([("suffix".to_owned(), json!(4))])
                ),
                &dataset
            )
            .expect("not suffix matches regex"),
            vec![false, true, false, false]
        );
    }

    #[test]
    fn evaluates_contained_by_operators() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "DOMAIN",
                    Operator::IsContainedBy,
                    ValueExpr::List(vec![json!("AE"), json!("CM")])
                ),
                &dataset
            )
            .expect("is contained by"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "DOMAIN",
                    Operator::IsNotContainedBy,
                    ValueExpr::List(vec![json!("AE"), json!("CM")])
                ),
                &dataset
            )
            .expect("is not contained by"),
            vec![false, false, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::IsContainedByCaseInsensitive,
                    ValueExpr::List(vec![json!("HEADACHE"), json!("NAUSEA")])
                ),
                &dataset
            )
            .expect("case-insensitive is contained by"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "TERM",
                    Operator::IsNotContainedByCaseInsensitive,
                    ValueExpr::List(vec![json!("HEADACHE"), json!("NAUSEA")])
                ),
                &dataset
            )
            .expect("case-insensitive is not contained by"),
            vec![false, false, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::IsContainedBy, literal("1|3")),
                &dataset
            )
            .expect("numeric is contained by pipe-delimited set"),
            vec![true, false, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::IsNotContainedBy, literal("1|3")),
                &dataset
            )
            .expect("numeric is not contained by pipe-delimited set"),
            vec![false, true, false, true]
        );
        assert!(scalar_contained_by_value(
            &ScalarValue::Number(1.01),
            &ScalarValue::String("1.01|2".to_owned()),
            false,
            false
        ));
        assert!(scalar_contains_all(
            &ScalarValue::String("AE|CM|DS".to_owned()),
            &ScalarValue::String("AE|CM".to_owned()),
            false
        ));
        assert!(!scalar_contains_all(
            &ScalarValue::String("AE|CM|DS".to_owned()),
            &ScalarValue::String("AE|LB".to_owned()),
            false
        ));
    }

    #[test]
    fn evaluates_is_not_ordered_subset_of_operator() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "meta.xpt",
      "domain": "META",
      "records": {
        "ORDER": ["STUDYID|DOMAIN|AETERM", "DOMAIN|STUDYID"],
        "MODEL": ["STUDYID|DOMAIN|AETERM|AESEQ", "STUDYID|DOMAIN"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition(
                    "ORDER",
                    Operator::IsNotOrderedSubsetOf,
                    ValueExpr::ColumnRef("MODEL".to_owned())
                ),
                &dataset
            )
            .expect("is not ordered subset of"),
            vec![false, true]
        );
    }

    #[test]
    fn evaluates_numeric_comparisons() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::LessThan, literal(3)),
                &dataset
            )
            .expect("less than"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::LessThanOrEqualTo, literal(3)),
                &dataset
            )
            .expect("less than or equal"),
            vec![true, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::GreaterThan, literal(1)),
                &dataset
            )
            .expect("greater than"),
            vec![false, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::GreaterThanOrEqualTo, literal(2)),
                &dataset
            )
            .expect("greater than or equal"),
            vec![false, true, true, false]
        );
    }

    #[test]
    fn evaluates_open_rules_date_comparisons_against_complete_dates() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::DateEqualTo, literal("2024-01-03")),
                &dataset
            )
            .expect("date equal"),
            vec![false, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::DateNotEqualTo, literal("2024-01-03")),
                &dataset
            )
            .expect("date not equal"),
            vec![true, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::DateLessThan, literal("2024-01-03")),
                &dataset
            )
            .expect("date less than"),
            vec![true, false, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "STARTDTC",
                    Operator::DateGreaterThanOrEqualTo,
                    literal("2024-01-03")
                ),
                &dataset
            )
            .expect("date greater than or equal"),
            vec![false, true, false, false]
        );
    }

    #[test]
    fn evaluates_open_rules_date_and_duration_validity_operators() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::IsCompleteDate, ValueExpr::Null),
                &dataset
            )
            .expect("complete date"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::IsIncompleteDate, ValueExpr::Null),
                &dataset
            )
            .expect("incomplete date"),
            vec![false, false, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("STARTDTC", Operator::InvalidDate, ValueExpr::Null),
                &dataset
            )
            .expect("invalid date"),
            vec![false, false, false, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("DUR", Operator::InvalidDuration, ValueExpr::Null),
                &dataset
            )
            .expect("invalid duration"),
            vec![false, false, false, true]
        );
    }

    #[test]
    fn evaluates_empty_string_date_as_incomplete_date() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBDTC": [""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition("LBDTC", Operator::IsIncompleteDate, ValueExpr::Null),
                &dataset
            )
            .expect("incomplete date"),
            vec![true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("LBDTC", Operator::IsCompleteDate, ValueExpr::Null),
                &dataset
            )
            .expect("complete date"),
            vec![false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("LBDTC", Operator::InvalidDate, ValueExpr::Null),
                &dataset
            )
            .expect("invalid date"),
            vec![false]
        );
    }

    #[test]
    fn treats_decimal_week_iso8601_duration_as_valid() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "timing.csv",
      "domain": "TIMING",
      "records": {
        "DUR": ["P4.5W", "P4,5W", "P4.W"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");

        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition("DUR", Operator::InvalidDuration, ValueExpr::Null),
                &dataset
            )
            .expect("invalid duration"),
            vec![false, false, true]
        );
    }

    #[test]
    fn incomplete_iso8601_dates_are_not_invalid_dates() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "TSSEQ": [1, 2, 3, 4, 5, 6, 7, 8, 9],
        "TSVAL": [
          "2003-12",
          "2003",
          "2003-12-15T13",
          "2003-12-15T-:15",
                "2003-12-15T13:-:17",
                "2003---15",
                "2013----14",
                "--12-15",
                "-----T07:15"
            ]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition("TSVAL", Operator::InvalidDate, ValueExpr::Null),
                &dataset
            )
            .expect("invalid date"),
            vec![false; 9]
        );
    }

    #[test]
    fn malformed_iso8601_datetime_suffix_is_invalid_date() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "TXSEQ": [1, 2],
        "TXVAL": ["2022-03-22T05-x", "2022-03-22T05:30"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition("TXVAL", Operator::InvalidDate, ValueExpr::Null),
                &dataset
            )
            .expect("invalid date"),
            vec![true, false]
        );
    }

    #[test]
    fn date_comparisons_order_incomplete_dates_by_known_prefix() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "RFSTDTC": ["2006-03", "2018-04-17", "2018-11"],
        "RFENDTC": ["2006-01-16", "2018-04", "2018"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RFSTDTC",
                    Operator::DateGreaterThan,
                    ValueExpr::ColumnRef("RFENDTC".to_owned())
                ),
                &dataset
            )
            .expect("date greater than"),
            vec![true, true, true]
        );
    }

    #[test]
    fn date_comparisons_accept_single_digit_datetime_hour() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "SVSTDTC": ["2019-01-07T6:10", "2019-01-07T06:09"],
        "SESTDTC": ["2019-01-07T06:10", "2019-01-07T06:10"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition(
                    "SVSTDTC",
                    Operator::DateGreaterThanOrEqualTo,
                    ValueExpr::ColumnRef("SESTDTC".to_owned())
                ),
                &dataset,
            )
            .expect("single digit hour comparison"),
            vec![true, false]
        );
    }

    #[test]
    fn evaluates_target_is_not_sorted_by_within_groups() {
        let dataset = sort_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "AESEQ",
                    Operator::TargetIsNotSortedBy,
                    ValueExpr::List(vec![json!({
                        "name": "AESTDTC",
                        "sort_order": "asc",
                        "null_position": "last"
                    })]),
                    serde_json::Map::from_iter([("within".to_owned(), json!("USUBJID"))])
                ),
                &dataset
            )
            .expect("target is not sorted by"),
            vec![false, true, true, false, false]
        );
    }

    #[test]
    fn evaluates_empty_within_except_last_row() {
        let dataset = end_date_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "SEENDTC",
                    Operator::EmptyWithinExceptLastRow,
                    literal("USUBJID"),
                    serde_json::Map::from_iter([("ordering".to_owned(), json!("SESTDTC"))])
                ),
                &dataset
            )
            .expect("empty within except last row"),
            vec![false, true, false, false]
        );
    }

    #[test]
    fn evaluates_does_not_have_next_corresponding_record() {
        let dataset = end_date_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "SEENDTC",
                    Operator::DoesNotHaveNextCorrespondingRecord,
                    literal("SESTDTC"),
                    serde_json::Map::from_iter([
                        ("ordering".to_owned(), json!("SESEQ")),
                        ("within".to_owned(), json!("USUBJID"))
                    ])
                ),
                &dataset
            )
            .expect("does not have next corresponding record"),
            vec![false, true, false, false]
        );
    }

    #[test]
    fn evaluates_not_present_on_multiple_rows_within() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "RELID",
                    Operator::NotPresentOnMultipleRowsWithin,
                    ValueExpr::Null,
                    serde_json::Map::from_iter([("within".to_owned(), json!("USUBJID"))])
                ),
                &dataset
            )
            .expect("not present on multiple rows within"),
            vec![false, false, true, true]
        );
    }

    #[test]
    fn evaluates_is_not_unique_set_within_columns() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RELID",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID")])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "RELID",
                    Operator::IsUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID")])
                ),
                &dataset
            )
            .expect("is unique set"),
            vec![false, false, true, true]
        );
    }

    #[test]
    fn unique_set_expands_dynamic_group_column_lists() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "eg.xpt",
      "domain": "EG",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ1"],
        "EGTESTCD": ["HR", "HR", "HR", "HR"],
        "VISIT": ["BASELINE", "BASELINE", "BASELINE", "BASELINE"],
        "EGDTC": ["2022-01-14", "2022-01-14T07:00", "2022-01-14", "2022-01-14"],
        "EGREPNUM": ["1", "2", "3", "1"],
        "$TIMING_VARIABLES": [
          "['VISIT', 'EGDTC']",
          "['VISIT', 'EGDTC']",
          "['VISIT', 'EGDTC']",
          "['VISIT', 'EGDTC']"
        ]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition(
                    "EGREPNUM",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![
                        json!("USUBJID"),
                        json!("EGTESTCD"),
                        json!("$TIMING_VARIABLES")
                    ])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, false, false, true]
        );
    }

    #[test]
    fn unique_set_applies_regex_to_dynamic_group_keys() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "eg.xpt",
      "domain": "EG",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "EGTESTCD": ["HR", "HR", "HR"],
        "VISIT": ["BASELINE", "BASELINE", "BASELINE"],
        "EGDTC": ["2022-01-14", "2022-01-14T07:00", "2022-01-14"],
        "EGREPNUM": ["1", "1", "1"],
        "$TIMING_VARIABLES": [
          "['VISIT', 'EGDTC']",
          "['VISIT', 'EGDTC']",
          "['VISIT', 'EGDTC']"
        ]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "EGREPNUM",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![
                        json!("USUBJID"),
                        json!("EGTESTCD"),
                        json!("$TIMING_VARIABLES")
                    ]),
                    serde_json::Map::from_iter([(
                        "regex".to_owned(),
                        json!(r"^\d{4}-\d{2}-\d{2}")
                    )])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, true, true]
        );
    }

    #[test]
    fn unique_set_treats_missing_group_columns_as_not_in_dataset() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RELID",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID"), json!("MISSING")])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn unique_set_treats_missing_target_column_as_not_in_dataset() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "MISSING",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID")])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition(
                    "MISSING",
                    Operator::IsUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID")])
                ),
                &dataset
            )
            .expect("is unique set"),
            vec![false, false, false, true]
        );
    }

    #[test]
    fn unique_set_includes_empty_target_values() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "TARGET_EMPTY_DUP",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![json!("GROUP_DUP")])
                ),
                &dataset
            )
            .expect("is not unique set"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn evaluates_is_not_unique_relationship_between_columns() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "LEFT",
                    Operator::IsNotUniqueRelationship,
                    ValueExpr::ColumnRef("RIGHT".to_owned())
                ),
                &dataset
            )
            .expect("is not unique relationship"),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn evaluates_is_not_unique_relationship_target_to_comparator_only() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "LEFT",
                    Operator::IsNotUniqueRelationship,
                    ValueExpr::ColumnRef("RIGHT".to_owned()),
                    serde_json::Map::from_iter([(
                        "direction".to_owned(),
                        json!("target_to_comparator")
                    )])
                ),
                &dataset
            )
            .expect("is not unique relationship"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn evaluates_is_not_unique_relationship_comparator_to_target_only() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition_with_options(
                    "LEFT",
                    Operator::IsNotUniqueRelationship,
                    ValueExpr::ColumnRef("RIGHT".to_owned()),
                    serde_json::Map::from_iter([(
                        "direction".to_owned(),
                        json!("comparator_to_target")
                    )])
                ),
                &dataset
            )
            .expect("is not unique relationship"),
            vec![false, false, true, true]
        );
    }

    #[test]
    fn evaluates_is_not_unique_relationship_with_empty_values() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "LEFT_EMPTY",
                    Operator::IsNotUniqueRelationship,
                    ValueExpr::ColumnRef("RIGHT_EMPTY".to_owned())
                ),
                &dataset
            )
            .expect("is not unique relationship"),
            vec![true, true, true, false]
        );
    }

    #[test]
    fn relationship_rule_uses_dataset_presence_preconditions() {
        let dataset = relationship_dataset();
        let rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::All(vec![
                ConditionGroup::Leaf(condition("VISITNUM", Operator::NotExists, ValueExpr::Null)),
                ConditionGroup::Leaf(condition(
                    "LEFT",
                    Operator::IsNotUniqueRelationship,
                    ValueExpr::ColumnRef("RIGHT".to_owned()),
                )),
            ]),
            "relationship failure",
        );

        assert!(validate_rule(&rule, &dataset)
            .expect("validate rule")
            .errors
            .is_empty());
    }

    #[test]
    fn evaluates_is_inconsistent_across_dataset_within_columns() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RELID",
                    Operator::IsInconsistentAcrossDataset,
                    ValueExpr::List(vec![json!("USUBJID")])
                ),
                &dataset
            )
            .expect("is inconsistent across dataset"),
            vec![true, true, true, false]
        );
    }

    #[test]
    fn inconsistent_across_dataset_treats_missing_group_columns_as_not_in_dataset() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RELID",
                    Operator::IsInconsistentAcrossDataset,
                    ValueExpr::List(vec![json!("USUBJID"), json!("MISSING")])
                ),
                &dataset
            )
            .expect("is inconsistent across dataset"),
            vec![true, true, true, false]
        );
    }

    #[test]
    fn inconsistent_across_dataset_includes_empty_target_values() {
        let dataset = relationship_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "RIGHT_EMPTY",
                    Operator::IsInconsistentAcrossDataset,
                    ValueExpr::List(vec![json!("LEFT")])
                ),
                &dataset
            )
            .expect("is inconsistent across dataset"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn evaluates_inconsistent_enumerated_columns() {
        let dataset = enumerated_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "COVAL",
                    Operator::InconsistentEnumeratedColumns,
                    ValueExpr::Null
                ),
                &dataset
            )
            .expect("inconsistent enumerated columns"),
            vec![false, true, false]
        );
    }

    #[test]
    fn null_values_are_detected_by_not_equal_and_ignored_by_ordering() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::NotEqualTo, literal("AE")),
                &dataset
            )
            .expect("null string not equal"),
            vec![false, true, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("DOMAIN", Operator::EqualTo, literal("AE")),
                &dataset
            )
            .expect("null string equal"),
            vec![true, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::GreaterThan, literal(1)),
                &dataset
            )
            .expect("null number greater than"),
            vec![false, true, true, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("AESEQ", Operator::LessThan, literal(3)),
                &dataset
            )
            .expect("null number less than"),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn evaluates_empty_and_not_empty() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::IsEmpty, ValueExpr::Null),
                &dataset
            )
            .expect("is empty"),
            vec![false, false, true, true]
        );
        assert_eq!(
            evaluate_condition(
                &condition("TERM", Operator::IsNotEmpty, ValueExpr::Null),
                &dataset
            )
            .expect("is not empty"),
            vec![true, true, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("MISSING", Operator::IsEmpty, ValueExpr::Null),
                &dataset
            )
            .expect("missing column is not empty"),
            vec![false, false, false, false]
        );
        assert_eq!(
            evaluate_condition(
                &condition("MISSING", Operator::IsNotEmpty, ValueExpr::Null),
                &dataset
            )
            .expect("missing column is not not-empty"),
            vec![false, false, false, false]
        );
    }

    #[test]
    fn evaluates_condition_groups() {
        let dataset = test_dataset();
        let group = ConditionGroup::All(vec![
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("AE"))),
            ConditionGroup::Not(Box::new(ConditionGroup::Leaf(condition(
                "TERM",
                Operator::IsEmpty,
                ValueExpr::Null,
            )))),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("group"),
            vec![true, false, false, false]
        );

        let group = ConditionGroup::Any(vec![
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("CM"))),
            ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(2))),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("any group"),
            vec![false, true, true, false]
        );
    }

    #[test]
    fn condition_groups_short_circuit_complete_boolean_masks() {
        let dataset = test_dataset();
        let group = ConditionGroup::Any(vec![
            ConditionGroup::Leaf(condition("MISSING", Operator::NotExists, ValueExpr::Null)),
            ConditionGroup::Leaf(condition("MISSING", Operator::NotEqualTo, literal("Y"))),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("any short-circuit"),
            vec![true, true, true, true]
        );

        let group = ConditionGroup::All(vec![
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("ZZ"))),
            ConditionGroup::Leaf(condition("MISSING", Operator::NotEqualTo, literal("Y"))),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("all short-circuit"),
            vec![false, false, false, false]
        );
    }

    #[test]
    fn any_condition_treats_missing_column_branch_as_false_when_another_branch_applies() {
        let dataset = test_dataset();
        let group = ConditionGroup::Any(vec![
            ConditionGroup::Leaf(condition("MISSING", Operator::NotEqualTo, literal("N"))),
            ConditionGroup::Leaf(condition("MISSING", Operator::NotExists, ValueExpr::Null)),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("condition group"),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn all_condition_treats_missing_column_branch_as_false() {
        let dataset = test_dataset();
        let group = ConditionGroup::All(vec![
            ConditionGroup::Leaf(condition("USUBJID", Operator::Exists, ValueExpr::Null)),
            ConditionGroup::Leaf(condition(
                "MISSING",
                Operator::MatchesRegex,
                literal("^.+$"),
            )),
        ]);

        assert_eq!(
            evaluate_condition_group(&group, &dataset).expect("condition group"),
            vec![false, false, false, false]
        );
    }

    #[test]
    fn all_condition_preserves_non_regex_missing_column_errors() {
        let dataset = test_dataset();
        let group = ConditionGroup::All(vec![
            ConditionGroup::Leaf(condition("USUBJID", Operator::Exists, ValueExpr::Null)),
            ConditionGroup::Leaf(condition("MISSING", Operator::EqualTo, literal("Y"))),
        ]);

        let error = evaluate_condition_group(&group, &dataset)
            .expect_err("non-regex missing columns should stay unsupported");

        assert!(matches!(error, EngineError::MissingColumn(_)));
    }

    #[test]
    fn missing_is_not_contained_by_target_is_false() {
        let dataset = test_dataset();

        assert_eq!(
            evaluate_condition(
                &condition(
                    "MISSING",
                    Operator::IsNotContainedBy,
                    ValueExpr::List(vec![json!("A")])
                ),
                &dataset,
            )
            .expect("missing target"),
            vec![false, false, false, false]
        );
    }

    #[test]
    fn type_insensitive_column_ref_equality_compares_numeric_strings_as_numbers() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBSTRESC": ["154", "200.00", "-44.0"],
        "LBSTRESN": [154.0, 200, -44]
      }
    }
  ]
}"#,
        )
        .expect("write dataset package");
        let dataset = load_dataset_package_json(&path)
            .expect("load dataset package")
            .into_iter()
            .next()
            .expect("dataset");

        let condition = condition_with_options(
            "LBSTRESC",
            Operator::NotEqualTo,
            ValueExpr::ColumnRef("LBSTRESN".to_owned()),
            serde_json::Map::from_iter([("type_insensitive".to_owned(), json!(true))]),
        );

        assert_eq!(
            evaluate_condition(&condition, &dataset).expect("type insensitive comparison"),
            vec![false, false, false]
        );
    }

    #[test]
    fn unsupported_operator_returns_error() {
        let dataset = test_dataset();
        let error = evaluate_condition(
            &condition(
                "DOMAIN",
                Operator::Unsupported("future_operator".to_owned()),
                literal("AE"),
            ),
            &dataset,
        )
        .expect_err("unsupported operator");

        assert!(matches!(error, EngineError::UnsupportedOperator(_)));
    }

    #[test]
    fn invalid_regex_returns_error() {
        let dataset = test_dataset();
        let error = evaluate_condition(
            &condition("TERM", Operator::MatchesRegex, literal("[")),
            &dataset,
        )
        .expect_err("invalid regex");

        assert!(matches!(error, EngineError::Regex(_)));
    }

    #[test]
    fn missing_target_returns_error() {
        let dataset = test_dataset();
        let error = evaluate_condition(
            &Condition {
                target: None,
                operator: Operator::EqualTo,
                comparator: literal("AE"),
                options: OperatorOptions::default(),
            },
            &dataset,
        )
        .expect_err("missing target");

        assert!(matches!(error, EngineError::MissingTarget));
    }

    #[test]
    fn extracts_target_variables_from_nested_conditions() {
        let group = ConditionGroup::All(vec![
            ConditionGroup::Leaf(condition("AESEQ", Operator::EqualTo, literal(1))),
            ConditionGroup::Leaf(condition(
                "AESEQ",
                Operator::EqualTo,
                ValueExpr::ColumnRef("AESEQ_COPY".to_owned()),
            )),
            ConditionGroup::Not(Box::new(ConditionGroup::Leaf(condition(
                "DOMAIN",
                Operator::EqualTo,
                literal("AE"),
            )))),
        ]);

        assert_eq!(
            extract_target_variables(&group),
            vec!["AESEQ", "AESEQ_COPY", "DOMAIN"]
        );
    }

    #[test]
    fn validate_rule_generates_record_level_issues() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition("DOMAIN", Operator::NotEqualTo, literal("AE"))),
            "DOMAIN must be AE",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.rule_id, "CORE-TEST-0001");
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.dataset, "AE");
        assert_eq!(result.domain.as_deref(), Some("AE"));
        assert_eq!(result.message, "DOMAIN must be AE");
        assert_eq!(result.error_count, 3);
        assert_eq!(result.errors.len(), 3);
        assert_eq!(result.errors[0].row, Some(2));
        assert_eq!(result.errors[0].variables, vec!["DOMAIN"]);
        assert_eq!(result.errors[0].message, "DOMAIN must be AE");
        assert_eq!(result.errors[0].usubjid.as_deref(), Some("SUBJ2"));
        assert_eq!(result.errors[0].seq.as_deref(), Some("2"));
        assert_eq!(result.errors[2].row, Some(4));
        assert_eq!(result.errors[2].usubjid.as_deref(), Some("SUBJ4"));
        assert_eq!(result.errors[2].seq, None);
    }

    #[test]
    fn validate_rule_generates_dataset_level_issue() {
        let dataset = test_dataset();
        let mut rule = rule(
            Some(Sensitivity::Dataset),
            ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(2))),
            "Dataset has AESEQ greater than 2",
        );
        rule.rule_type = RuleType::DatasetMetadata;

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].row, None);
        assert_eq!(result.errors[0].variables, vec!["AESEQ"]);
        assert_eq!(result.errors[0].usubjid, None);
        assert_eq!(result.errors[0].seq, None);
    }

    #[test]
    fn record_data_with_dataset_sensitivity_reports_matching_records() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Dataset),
            ConditionGroup::Leaf(condition("DOMAIN", Operator::Exists, ValueExpr::Null)),
            "DOMAIN variable is present",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 4);
        assert_eq!(
            result
                .errors
                .iter()
                .map(|issue| issue.row)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3), Some(4)]
        );
        assert_eq!(result.errors[0].variables, vec!["DOMAIN"]);
        assert_eq!(result.errors[0].usubjid.as_deref(), Some("SUBJ1"));
        assert_eq!(result.errors[0].seq.as_deref(), Some("1"));
    }

    #[test]
    fn record_data_with_dataset_sensitivity_treats_exists_as_column_presence() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Dataset),
            ConditionGroup::Leaf(condition("TERM", Operator::Exists, ValueExpr::Null)),
            "TERM variable is present",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 4);
        assert_eq!(result.errors[3].row, Some(4));
    }

    #[test]
    fn record_data_with_unique_set_treats_exists_as_column_presence() {
        let dataset = relationship_dataset();
        let rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::All(vec![
                ConditionGroup::Leaf(condition(
                    "TARGET_EMPTY_DUP",
                    Operator::Exists,
                    ValueExpr::Null,
                )),
                ConditionGroup::Leaf(condition(
                    "TARGET_EMPTY_DUP",
                    Operator::IsEmpty,
                    ValueExpr::Null,
                )),
                ConditionGroup::Leaf(condition(
                    "GROUP_DUP",
                    Operator::IsNotUniqueSet,
                    ValueExpr::List(vec![json!("USUBJID")]),
                )),
            ]),
            "empty target participates in duplicate-set rules",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(
            result
                .errors
                .iter()
                .map(|issue| issue.row)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2)]
        );
    }

    #[test]
    fn validation_issues_expand_domain_prefixed_placeholder_variables() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Dataset),
            ConditionGroup::All(vec![
                ConditionGroup::Leaf(condition("--MISSING", Operator::NotExists, ValueExpr::Null)),
                ConditionGroup::Leaf(condition("--SEQ", Operator::Exists, ValueExpr::Null)),
            ]),
            "prefixed variable issue",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 4);
        assert_eq!(result.errors[0].variables, vec!["AEMISSING", "AESEQ"]);
    }

    #[test]
    fn validation_issues_prefer_rule_output_variables() {
        let dataset = test_dataset();
        let mut rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition("DOMAIN", Operator::NotEqualTo, literal("AE"))),
            "DOMAIN must be AE",
        );
        rule.output_variables = vec!["TERM".to_owned(), "--SEQ".to_owned(), "DOMAIN".to_owned()];

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.errors[0].variables, vec!["TERM", "AESEQ", "DOMAIN"]);
    }

    #[test]
    fn empty_within_except_last_row_reports_only_target_variable() {
        let dataset = end_date_dataset();
        let mut rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition_with_options(
                "SEENDTC",
                Operator::EmptyWithinExceptLastRow,
                literal("USUBJID"),
                serde_json::Map::from_iter([("ordering".to_owned(), json!("SESTDTC"))]),
            )),
            "SEENDTC is empty before the last row",
        );
        rule.output_variables = vec!["SESTDTC".to_owned(), "SEENDTC".to_owned()];

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.errors[0].variables, vec!["SEENDTC"]);
    }

    #[test]
    fn not_present_on_multiple_rows_reports_within_and_target_variables() {
        let dataset = relationship_dataset();
        let mut rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition_with_options(
                "RELID",
                Operator::NotPresentOnMultipleRowsWithin,
                ValueExpr::Null,
                serde_json::Map::from_iter([("within".to_owned(), json!("USUBJID"))]),
            )),
            "RELID must appear on multiple rows",
        );
        rule.output_variables = vec!["RELID".to_owned()];

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.errors[0].variables, vec!["USUBJID", "RELID"]);
    }

    #[test]
    fn validate_rule_passes_when_no_mask_values_are_true() {
        let dataset = test_dataset();
        let rule = rule(
            Some(Sensitivity::Record),
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("ZZ"))),
            "DOMAIN must not be ZZ",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Passed);
        assert_eq!(result.error_count, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_rule_requires_record_or_dataset_sensitivity() {
        let dataset = test_dataset();
        let missing_sensitivity_rule = rule(
            None,
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("AE"))),
            "message",
        );

        let error =
            validate_rule(&missing_sensitivity_rule, &dataset).expect_err("missing sensitivity");
        assert!(matches!(error, EngineError::MissingSensitivity));

        let unsupported_rule = rule(
            Some(Sensitivity::Study),
            ConditionGroup::Leaf(condition("DOMAIN", Operator::EqualTo, literal("AE"))),
            "message",
        );

        let error =
            validate_rule(&unsupported_rule, &dataset).expect_err("unsupported sensitivity");
        assert!(matches!(error, EngineError::UnsupportedSensitivity(_)));
    }
}
