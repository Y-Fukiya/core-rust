use core_rule_model::{Condition, Operator, OperatorOptions};

use super::super::*;
use super::common::{condition, literal, test_dataset};

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
