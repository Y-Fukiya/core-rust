use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_executes_target_is_not_sorted_by_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SORT-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-SORT-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "AESTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "AESEQ is not chronological" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "AESEQ": [1, 3, 2],
        "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_executes_empty_within_except_last_row_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-END-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-END-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SEENDTC",
    "operator": "empty_within_except_last_row",
    "ordering": "SESTDTC",
    "value": "USUBJID"
  },
  "Outcome": { "Message": "SEENDTC is empty before the last row" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SESEQ": [1, 2, 3],
        "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"],
        "SEENDTC": ["2024-01-02", "", ""]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_not_present_on_multiple_rows_within_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-REL-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-REL-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "not_present_on_multiple_rows_within",
    "within": "USUBJID"
  },
  "Outcome": { "Message": "RELID must appear on multiple rows" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_is_not_unique_set_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-UNIQUE-SET.json"),
        r#"{
  "Core": { "Id": "CORE-UNIQUE-SET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be unique within subject" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}
