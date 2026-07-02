use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_blinding_schema_masked_roles_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001072.json"),
            r##"{
  "Core": { "Id": "CORE-001072", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a blinding schema that is not open label or double blind but there is no applicable study role that is masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1"]
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "No masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Has masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_3",
            "name": "Open label",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
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
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "InterventionalStudyDesign"
    );
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "name",
            "blindingSchema.code",
            "blindingSchema.decode",
            "# Masked Roles",
            "Applicable Roles"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_double_blind_requires_two_masked_roles_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001071.json"),
            r##"{
  "Core": { "Id": "CORE-001071", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a double blind blinding schema but there are not at least two applicable study roles that are masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_3",
            "instanceType": "StudyRole",
            "code": { "decode": "Assessor" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Only one masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Two masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
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
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "InterventionalStudyDesign"
    );
}

#[test]
fn run_validation_executes_usdm_open_label_rejects_masked_role_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001070.json"),
            r##"{
  "Core": { "Id": "CORE-001070", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles.{\"check\": true}",
  "Outcome": {
    "Message": "A masking is defined for the study role, but the role applies to a study design with an open label blinding schema.",
    "Output Variables": ["name", "code", "masking.text", "masking.isMasked", "appliesToIds", "StudyDesign.id", "StudyDesign.blindingSchema"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Masked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Masked", "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "name": "Unmasked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Not masked", "isMasked": false }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Open label design",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
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
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyRoleBlinding");
}
