use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_json_schema_check_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000935.json"),
        r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": 71476, "decode": "Approval Date" }
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "population": {
              "plannedAge": {
                "isApproximate": "false"
              }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "JSONSchemaIssue");
}

#[test]
fn run_validation_executes_usdm_json_schema_check_rules_with_no_issues() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000935.json"),
        r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": "C71476", "decode": "Approval Date" },
            "geographicScopes": [
              { "id": "GeographicScope_1", "code": null }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}
