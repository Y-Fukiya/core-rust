use std::fs;

use core_data::{load_dataset_package_json, LoadedDataset};
use core_rule_model::{ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity, ValueExpr};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use super::common::{condition, literal, rule};
use crate::{validate_rule, ExecutionStatus};

fn ae_dataset(aestat: Option<serde_json::Value>) -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    let mut records = json!({
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "AESEQ": [1, 2, 3],
        "DOMAIN": ["AE", "AE", "AE"]
    });
    if let Some(values) = aestat {
        records["AESTAT"] = values;
    }
    fs::write(
        &path,
        serde_json::to_vec(&json!({"datasets": [{
            "filename": "ae.csv", "domain": "AE", "records": records
        }]}))
        .expect("serialize dataset"),
    )
    .expect("write dataset");
    load_dataset_package_json(&path)
        .expect("load dataset")
        .remove(0)
}

fn aestat_presence_rule(sensitivity: Sensitivity) -> ExecutableRule {
    let mut rule = rule(
        Some(sensitivity),
        ConditionGroup::All(vec![ConditionGroup::Leaf(condition(
            "AESTAT",
            Operator::Exists,
            ValueExpr::Null,
        ))]),
        "AESTAT must not be present in AE dataset.",
    );
    rule.core_id = "CORE-000013".to_owned();
    rule.output_variables = vec!["AESTAT".to_owned()];
    rule
}

#[test]
fn not_equal_length_detects_short_long_and_missing_text() {
    let rule = rule(
        Some(Sensitivity::Record),
        ConditionGroup::Leaf(condition(
            "AESTAT",
            Operator::from_name("has_not_equal_length"),
            literal(json!(2)),
        )),
        "Unexpected length",
    );
    let result = validate_rule(&rule, &ae_dataset(Some(json!(["A", "ABC", null]))))
        .expect("validate lengths");
    assert_eq!(result.error_count, 3);

    let result = validate_rule(&rule, &ae_dataset(Some(json!(["AE", "éA", "A "]))))
        .expect("validate two-character values");
    assert_eq!(result.execution_status, ExecutionStatus::Passed);
}

#[test]
fn dataset_sensitivity_reports_one_issue_for_multiple_matching_records() {
    let result = validate_rule(
        &aestat_presence_rule(Sensitivity::Dataset),
        &ae_dataset(Some(json!(["NOT DONE", "NOT DONE", "NOT DONE"]))),
    )
    .expect("validate CORE-000013");

    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors.len(), 1);
    let issue = &result.errors[0];
    assert_eq!(issue.rule_id, "CORE-000013");
    assert_eq!(issue.dataset, "AE");
    assert_eq!(issue.domain.as_deref(), Some("AE"));
    assert_eq!(issue.variables, vec!["AESTAT"]);
    assert_eq!(issue.row, None);
    assert_eq!(issue.usubjid, None);
    assert_eq!(issue.seq, None);
}

#[test]
fn dataset_sensitivity_blank_present_column_still_exists() {
    let result = validate_rule(
        &aestat_presence_rule(Sensitivity::Dataset),
        &ae_dataset(Some(json!(["", null, " "]))),
    )
    .expect("validate blank AESTAT");

    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].variables, vec!["AESTAT"]);
    assert_eq!(result.errors[0].row, None);
}

#[test]
fn dataset_sensitivity_missing_forbidden_column_passes() {
    let result = validate_rule(
        &aestat_presence_rule(Sensitivity::Dataset),
        &ae_dataset(None),
    )
    .expect("validate absent AESTAT");

    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn dataset_sensitivity_record_control_keeps_matching_row_locators() {
    let result = validate_rule(
        &aestat_presence_rule(Sensitivity::Record),
        &ae_dataset(Some(json!(["NOT DONE", null, "NOT DONE"]))),
    )
    .expect("validate record sensitivity");

    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(result.dataset, "AE");
    assert_eq!(result.domain.as_deref(), Some("AE"));
    assert_eq!(result.errors[0].dataset, "AE");
    assert_eq!(result.errors[0].domain.as_deref(), Some("AE"));
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(result.errors[0].usubjid.as_deref(), Some("SUBJ1"));
    assert_eq!(result.errors[0].seq.as_deref(), Some("1"));
    assert_eq!(result.errors[1].row, Some(3));
    assert_eq!(result.errors[1].usubjid.as_deref(), Some("SUBJ3"));
    assert_eq!(result.errors[1].seq.as_deref(), Some("3"));
}

#[test]
fn dataset_sensitivity_aggregates_a_partial_failure_mask() {
    let rule = rule(
        Some(Sensitivity::Dataset),
        ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(1))),
        "AESEQ is greater than 1",
    );
    let result = validate_rule(&rule, &ae_dataset(None)).expect("validate partial mask");

    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn dataset_sensitivity_domain_presence_reports_once_without_physical_locators() {
    let dataset = ae_dataset(None);
    let mut rule = rule(
        Some(Sensitivity::Dataset),
        ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(1))),
        "Domain presence finding",
    );
    rule.rule_type = RuleType::DomainPresence;

    let result = validate_rule(&rule, &dataset).expect("validate domain presence");

    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.dataset, "STUDY");
    assert_eq!(result.domain, None);
    let issue = &result.errors[0];
    assert_eq!(issue.rule_id, "CORE-TEST-0001");
    assert_eq!(issue.dataset, "STUDY");
    assert_eq!(issue.domain, None);
    assert_eq!(issue.variables, vec!["AESEQ"]);
    assert_eq!(issue.row, None);
    assert_eq!(issue.usubjid, None);
    assert_eq!(issue.seq, None);
    assert_eq!(dataset.metadata().name, "AE");
    assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
    assert_eq!(dataset.frame().height(), 3);
}

#[test]
fn domain_presence_reporting_preserves_record_and_group_counts() {
    for sensitivity in [Sensitivity::Record, Sensitivity::Group] {
        let mut rule = rule(
            Some(sensitivity),
            ConditionGroup::Leaf(condition("AESEQ", Operator::GreaterThan, literal(1))),
            "Domain presence finding",
        );
        rule.rule_type = RuleType::DomainPresence;

        let result = validate_rule(&rule, &ae_dataset(None)).expect("validate domain presence");

        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 2);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.dataset, "STUDY");
        assert_eq!(result.domain, None);
        for issue in &result.errors {
            assert_eq!(issue.dataset, "STUDY");
            assert_eq!(issue.domain, None);
            assert_eq!(issue.row, None);
            assert_eq!(issue.usubjid, None);
            assert_eq!(issue.seq, None);
        }
    }
}

#[test]
fn group_sensitivity_uses_declared_keys_and_reports_the_first_failing_record() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{"datasets": [{
            "filename": "lb.csv", "domain": "LB", "records": {
                "USUBJID": ["S1", "S2", "S3", "S4"],
                "LBSEQ": [1, 2, 3, 4],
                "lbtestcd": ["A", "A", "A", "B"],
                "LBSTAT": ["NOT DONE", "", "", ""],
                "LBSTRESU": ["cm", "mm", "cm", "mm"]
            }
        }]}"#,
    )
    .expect("write grouped findings");
    let dataset = load_dataset_package_json(&path)
        .expect("load dataset")
        .remove(0);
    assert!(dataset.frame().column("lbtestcd").is_ok());
    let rule = core_rule_model::normalize_rule(json!({
        "Core": {"Id": "CORE-TEST-GROUP"},
        "Rule Type": "Record Data",
        "Sensitivity": "Group",
        "Grouping_Variables": ["--TESTCD", "--METHOD"],
        "Check": {"all": [{
            "name": "--STRESU",
            "operator": "is_inconsistent_across_dataset",
            "value": ["--TESTCD", "--METHOD", "--STAT"]
        }]},
        "Outcome": {
            "Message": "Inconsistent units",
            "Output Variables": ["--TESTCD", "--METHOD", "--STAT", "--STRESU"]
        }
    }))
    .expect("normalize grouped findings rule");

    let result = validate_rule(&rule, &dataset).expect("validate grouped findings");
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    let issue = &result.errors[0];
    assert_eq!(issue.row, Some(2));
    assert_eq!(issue.usubjid.as_deref(), Some("S2"));
    assert_eq!(issue.seq.as_deref(), Some("2"));
    assert_eq!(
        issue.variables,
        ["LBTESTCD", "LBMETHOD", "LBSTAT", "LBSTRESU"]
    );
}

#[test]
fn dataset_sensitivity_domain_presence_without_matches_keeps_study_identity() {
    let mut rule = aestat_presence_rule(Sensitivity::Dataset);
    rule.core_id = "CORE-TEST-DOMAIN-PRESENCE".to_owned();
    rule.rule_type = RuleType::DomainPresence;

    let result = validate_rule(&rule, &ae_dataset(None)).expect("validate absent target");

    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
    assert!(result.errors.is_empty());
    assert_eq!(result.dataset, "STUDY");
    assert_eq!(result.domain, None);
}
