#![forbid(unsafe_code)]

use core_data::LoadedDataset;
use core_rule_model::{
    ActionSpec, Condition, ConditionGroup, ExecutableRule, Operator, Sensitivity, ValueExpr,
};
use polars::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, EngineError>;
pub type BooleanMask = Vec<bool>;

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
    UnsupportedRuleType,
    UnsupportedOperator,
    OperationsNotSupported,
    JsonataNotSupported,
    DatasetJoinNotSupported,
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
    let mask = evaluate_rule_conditions(rule, dataset)?;
    let message =
        outcome_message(&rule.actions).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let variables = extract_target_variables(&rule.conditions);

    let errors = match rule
        .sensitivity
        .as_ref()
        .ok_or(EngineError::MissingSensitivity)?
    {
        Sensitivity::Record => record_level_issues(rule, dataset, &mask, &variables, &message)?,
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
    let row_count = dataset.frame().height();

    match group {
        ConditionGroup::All(groups) => {
            let mut mask = vec![true; row_count];
            for group in groups {
                and_assign(&mut mask, &evaluate_condition_group(group, dataset)?);
            }
            Ok(mask)
        }
        ConditionGroup::Any(groups) => {
            let mut mask = vec![false; row_count];
            for group in groups {
                or_assign(&mut mask, &evaluate_condition_group(group, dataset)?);
            }
            Ok(mask)
        }
        ConditionGroup::Not(group) => Ok(evaluate_condition_group(group, dataset)?
            .into_iter()
            .map(|value| !value)
            .collect()),
        ConditionGroup::Leaf(condition) => evaluate_condition(condition, dataset),
    }
}

pub fn extract_target_variables(group: &ConditionGroup) -> Vec<String> {
    let mut variables = Vec::new();
    collect_target_variables(group, &mut variables);
    variables
}

pub fn evaluate_condition(condition: &Condition, dataset: &LoadedDataset) -> Result<BooleanMask> {
    let frame = dataset.frame();
    let row_count = frame.height();
    let operator = &condition.operator;
    let target = condition
        .target
        .as_deref()
        .ok_or(EngineError::MissingTarget)?;

    match operator {
        Operator::Exists => {
            let Some(column) = optional_column(frame, target)? else {
                return Ok(vec![false; row_count]);
            };
            return evaluate_column(column, row_count, |value, _row| Ok(!value.is_null()));
        }
        Operator::NotExists => {
            let Some(column) = optional_column(frame, target)? else {
                return Ok(vec![true; row_count]);
            };
            return evaluate_column(column, row_count, |value, _row| Ok(value.is_null()));
        }
        _ => {}
    }

    let column = frame
        .column(target)
        .map_err(|_| EngineError::MissingColumn(target.to_owned()))?;

    match operator {
        Operator::EqualTo | Operator::NotEqualTo => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_comparator(comparator, frame, row)?;
                let equal = scalar_equal(&left, &right);
                Ok(matches!(operator, Operator::EqualTo) == equal)
            })
        }
        Operator::Contains | Operator::DoesNotContain => {
            let needle = string_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, _row| {
                let contains = ScalarValue::from_any_value(value)
                    .into_string()
                    .map(|haystack| haystack.contains(&needle))
                    .unwrap_or(false);
                Ok(matches!(operator, Operator::Contains) == contains)
            })
        }
        Operator::LessThan
        | Operator::LessThanOrEqualTo
        | Operator::GreaterThan
        | Operator::GreaterThanOrEqualTo => {
            let comparator = required_comparator(operator, &condition.comparator)?;
            evaluate_column(column, row_count, |value, row| {
                let left = ScalarValue::from_any_value(value);
                let right = resolve_scalar_comparator(comparator, frame, row)?;
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
        Operator::IsEmpty | Operator::IsNotEmpty => {
            evaluate_column(column, row_count, |value, _row| {
                let empty = ScalarValue::from_any_value(value).is_empty();
                Ok(matches!(operator, Operator::IsEmpty) == empty)
            })
        }
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
                row: Some(row + 1),
                variables: variables.to_vec(),
                message: message.to_owned(),
                usubjid: cell_string(dataset.frame(), "USUBJID", row)?,
                seq: sequence_value(dataset, row)?,
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
        Err(PolarsError::ColumnNotFound(_)) => Ok(None),
        Err(source) => Err(source.into()),
    }
}

fn required_comparator<'a>(
    operator: &Operator,
    comparator: &'a ValueExpr,
) -> Result<&'a ValueExpr> {
    if matches!(comparator, ValueExpr::Null) {
        match operator {
            Operator::EqualTo | Operator::NotEqualTo => Ok(comparator),
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

fn resolve_scalar_comparator(
    comparator: &ValueExpr,
    frame: &DataFrame,
    row: usize,
) -> Result<ScalarValue> {
    match comparator {
        ValueExpr::Literal(value) => Ok(json_value_to_scalar(value)),
        ValueExpr::Null => Ok(ScalarValue::Null),
        ValueExpr::ColumnRef(column_name) => {
            let column = frame
                .column(column_name)
                .map_err(|_| EngineError::MissingColumn(column_name.clone()))?;
            Ok(ScalarValue::from_any_value(column.get(row)?))
        }
        ValueExpr::List(_) => Err(EngineError::InvalidComparator {
            operator: "scalar_comparison".to_owned(),
            comparator: comparator.clone(),
        }),
    }
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
            Self::String(value) => value.parse().ok(),
            Self::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Self::Null => None,
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

fn scalar_equal(left: &ScalarValue, right: &ScalarValue) -> bool {
    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => true,
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => left == right,
        _ => match (left.as_number(), right.as_number()) {
            (Some(left), Some(right)) => left == right,
            _ => left.as_string() == right.as_string(),
        },
    }
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

    fn condition(target: &str, operator: Operator, comparator: ValueExpr) -> Condition {
        Condition {
            target: Some(target.to_owned()),
            operator,
            comparator,
            options: OperatorOptions::default(),
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
        let rule = rule(
            Some(Sensitivity::Dataset),
            ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(2))),
            "Dataset has AESEQ greater than 2",
        );

        let result = validate_rule(&rule, &dataset).expect("validate rule");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].row, None);
        assert_eq!(result.errors[0].variables, vec!["AESEQ"]);
        assert_eq!(result.errors[0].usubjid, None);
        assert_eq!(result.errors[0].seq, None);
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
