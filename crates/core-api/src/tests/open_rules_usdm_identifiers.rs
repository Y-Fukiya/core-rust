use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_id_contains_space_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001075.json"),
            r#"{
  "Core": { "Id": "CORE-001075", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": "(**[$contains($string(id),\" \")])@$i.{\"instanceType\": $i.instanceType,\"id\": $join(['\"','\"'],$i.id),\"path\": $i._path,\"name\": $i.name}",
  "Outcome": {
    "Message": "The id value contains a space.",
    "Output Variables": ["name"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "id": "Study 1",
    "name": "Study with spaced id",
    "instanceType": "Study",
    "versions": [
      {
        "id": "StudyVersion_1",
        "name": "Clean version",
        "instanceType": "StudyVersion"
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
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "USDMObject");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["name"]);
}

#[test]
fn run_validation_executes_usdm_study_identifier_duplicate_scope_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000956.json"),
            r#"{
  "Core": { "Id": "CORE-000956", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyIdentifier"] } },
  "Check": "study.versions@$sv.($sv.organizations{id:($o:=$;$i:=$sv.studyIdentifiers[scopeId=$o.id];$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "More than 1 study identifier is specified for the same organization.",
    "Output Variables": ["text", "scopeId", "Organization.name"]
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
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "studyIdentifiers": [
          { "id": "StudyIdentifier_1", "instanceType": "StudyIdentifier", "text": "ABC-001", "scopeId": "Organization_1" },
          { "id": "StudyIdentifier_2", "instanceType": "StudyIdentifier", "text": "NCT-001", "scopeId": "Organization_1" }
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyIdentifier");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["text", "scopeId", "Organization.name"]
    );
}

#[test]
fn run_validation_executes_usdm_identifier_text_duplicate_scope_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000955.json"),
            r#"{
  "Core": { "Id": "CORE-000955", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ReferenceIdentifier"] } },
  "Check": "study.versions@$sv.($sv.**.*[scopeId and text and instanceType]{$join([text,scopeId,instanceType],\"\\n\"):($i:=$;$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "The identifier text is not unique within the scope of the identified organization.",
    "Output Variables": ["text", "scopeId", "Organization.name", "type.decode"]
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
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "referenceIdentifiers": [
          {
            "id": "ReferenceIdentifier_1",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Clinical Development Plan" }
          },
          {
            "id": "ReferenceIdentifier_2",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Pediatric Investigation Plan" }
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
    assert_eq!(outcome.results[0].errors[0].dataset, "ReferenceIdentifier");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["text", "scopeId", "Organization.name", "type.decode"]
    );
}
