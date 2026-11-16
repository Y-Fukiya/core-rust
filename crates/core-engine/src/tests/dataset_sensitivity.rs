use std::fs;

use core_data::{load_dataset_package_json, LoadedDataset};
use core_rule_model::{ConditionGroup, ExecutableRule, Operator, Sensitivity, ValueExpr};
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
