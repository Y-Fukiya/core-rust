use std::fs;

use core_engine::ExecutionStatus;
use core_rule_model::load_rules_from_paths;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;
use crate::tests::helpers::{write_dataset, write_rule};

#[test]
fn run_validation_records_engine_errors_as_skipped_results() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-MISSING-COLUMN.json"),
        r#"{
  "Core": { "Id": "CORE-MISSING-COLUMN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESTDTC",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "AESTDTC must be populated" }
}"#,
    )
    .expect("write missing column rule");
    write_rule(&rules_dir, "CORE-VALID", "AE");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let skipped = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-MISSING-COLUMN")
        .expect("skipped missing column result");
    assert_eq!(skipped.execution_status, ExecutionStatus::Skipped);
    assert_eq!(skipped.skipped_reason, Some(SkippedReason::EvaluationError));
    assert_eq!(skipped.dataset, "AE");
    assert!(skipped
        .message
        .contains("dataset is missing required column"));

    let valid = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-VALID")
        .expect("valid rule result");
    assert_eq!(valid.execution_status, ExecutionStatus::Failed);
}

#[test]
fn preflight_accepts_is_not_unique_relationship_operator() {
    assert!(is_supported_basic_operator(
        &Operator::IsNotUniqueRelationship
    ));
}

#[test]
fn select_rules_includes_only_requested_ids_and_skips_missing_ids() {
    let dir = tempdir().expect("tempdir");
    write_rule(dir.path(), "CORE-TEST-0001", "AE");
    write_rule(dir.path(), "CORE-TEST-0002", "CM");
    let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

    let selection = select_rules(
        &rules,
        &["CORE-TEST-0002".to_owned(), "CORE-MISSING".to_owned()],
        &[],
    )
    .expect("select rules");

    assert_eq!(selection.selected.len(), 1);
    assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
    assert_eq!(selection.skipped.len(), 1);
    assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
    assert_eq!(
        selection.skipped[0].execution_status,
        ExecutionStatus::Skipped
    );
}

#[test]
fn select_rules_excludes_requested_ids_and_skips_missing_exclusions() {
    let dir = tempdir().expect("tempdir");
    write_rule(dir.path(), "CORE-TEST-0001", "AE");
    write_rule(dir.path(), "CORE-TEST-0002", "CM");
    let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

    let selection = select_rules(
        &rules,
        &[],
        &["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
    )
    .expect("select rules");

    assert_eq!(selection.selected.len(), 1);
    assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
    assert_eq!(selection.skipped.len(), 1);
    assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
}

#[test]
fn select_rules_rejects_include_and_exclude_together() {
    let error = select_rules(
        &[],
        &["CORE-TEST-0001".to_owned()],
        &["CORE-TEST-0002".to_owned()],
    )
    .expect_err("mutually exclusive filters");

    assert!(matches!(error, ApiError::MutuallyExclusiveRuleFilters));
}

#[test]
fn run_validation_filters_rules_and_writes_reports() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_rule(&rules_dir, "CORE-TEST-0001", "AE");
    write_rule(&rules_dir, "CORE-TEST-0002", "CM");
    let dataset_path = write_dataset(&data_dir);

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        include_rules: vec!["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
        exclude_rules: Vec::new(),
        output_dir: Some(output_dir.clone()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(outcome.results[0].rule_id, "CORE-MISSING");
    assert_eq!(outcome.results[1].rule_id, "CORE-TEST-0001");
    assert_eq!(outcome.results[1].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[1].error_count, 1);
    assert!(outcome
        .reports
        .expect("reports")
        .json
        .expect("json report")
        .exists());
    assert!(output_dir.join("report.csv").exists());
}
