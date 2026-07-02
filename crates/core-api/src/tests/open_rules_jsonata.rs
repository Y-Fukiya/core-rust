use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use tempfile::tempdir;

use crate::tests::helpers::{write_dataset, write_raw_rule};
use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_skips_unsupported_rules_before_loading_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::write(
        rules_dir.join("CORE-JSONATA-UNSUPPORTED.json"),
        r#"{
  "Core": { "Id": "CORE-JSONATA-UNSUPPORTED", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$.study.versions.studyDesigns.{\"id\": id}[id != null]",
  "Outcome": { "Message": "Unsupported JSONata" }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dir.path().join("missing-data")],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        output_dir: Some(output_dir.clone()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::UnsupportedOperator)
    );
    let report_csv = fs::read_to_string(output_dir.join("report.csv")).expect("read csv");
    assert!(report_csv.contains("CORE-JSONATA-UNSUPPORTED"));
    assert!(report_csv.contains("unsupported_operator"));
}

#[test]
fn run_validation_executes_jsonata_rules_when_conditions_are_normalized() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    write_raw_rule(
        &rules_dir,
        "CORE-JSONATA",
        r#""Rule Type": "JSONATA""#,
        "",
        r#""operator": "not_equal_to""#,
    );

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-JSONATA");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["DOMAIN".to_owned()]
    );
}

#[test]
fn run_validation_executes_jsonata_string_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-JSONATA-STRING.json"),
        r#"{
  "Core": { "Id": "CORE-JSONATA-STRING", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$exists(DOMAIN) and DOMAIN != 'AE'",
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write jsonata string rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}
