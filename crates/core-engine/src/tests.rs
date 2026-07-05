use std::fs;

use core_data::load_dataset_package_json;
use core_rule_model::{
    Condition, ConditionGroup, Operator, OperatorOptions, RuleType, Sensitivity, ValueExpr,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use super::*;
mod common;

use common::{
    condition, condition_with_options, end_date_dataset, enumerated_dataset, literal,
    relationship_dataset, rule, sort_dataset, test_dataset,
};

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
                serde_json::Map::from_iter([("regex".to_owned(), json!(r"^\d{4}-\d{2}-\d{2}"))])
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

    let error = validate_rule(&unsupported_rule, &dataset).expect_err("unsupported sensitivity");
    assert!(matches!(error, EngineError::UnsupportedSensitivity(_)));
}
