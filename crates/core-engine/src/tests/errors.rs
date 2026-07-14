use core_rule_model::{Condition, ConditionGroup, Operator, OperatorOptions, ValueExpr};

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
