use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_filters_execution_datasets_by_entity_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-SCOPE", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "instanceType",
    "operator": "equal_to",
    "value": "StudyEpoch",
    "value_is_literal": true
  },
  "Outcome": { "Message": "StudyEpoch rows are checked once" }
}"#,
    )
    .expect("write entity scope rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "id": ["StudyEpoch_1"],
        "instanceType": ["StudyEpoch"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "StudyEpoch");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_skips_entity_scope_column_ref_comparators() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-COLUMN-REF.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-COLUMN-REF", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "nextId",
    "operator": "not_equal_to",
    "value": "parent_id"
  },
  "Outcome": { "Message": "Entity relationship comparisons need entity semantics" }
}"#,
    )
    .expect("write entity column-ref rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "nextId": ["StudyEpoch_2"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
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
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_executes_entity_scope_missing_column_ref_literal_fallback() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-LITERAL-FALLBACK.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-LITERAL-FALLBACK", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "rel_type",
    "operator": "equal_to",
    "value": "definition"
  },
  "Outcome": { "Message": "definition activities are checked" }
}"#,
    )
    .expect("write entity literal fallback rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1", "Activity_2"],
        "rel_type": ["definition", "instance"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_reports_entity_literal_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000820.json"),
        r#"{
  "Core": { "Id": "CORE-000820", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "type",
    "operator": "equal_to",
    "value": "anchor"
  },
  "Outcome": { "Message": "entity literal oracle semantics are not supported" }
}"#,
    )
    .expect("write entity oracle gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "id": ["Timing_1"],
        "type": ["anchor"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}
