use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_abbreviation_expanded_text_duplicate_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001067.json"),
            r##"{
  "Core": { "Id": "CORE-001067", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's expanded text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
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
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "Cu",
            "expandedText": "copper"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "LBC",
            "expandedText": "Copper"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyVersion.id",
            "StudyVersion.versionIdentifier",
            "abbreviatedText",
            "expandedText"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_abbreviation_text_duplicate_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001053.json"),
            r##"{
  "Core": { "Id": "CORE-001053", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's abbreviated text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
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
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse experience"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "BMI",
            "expandedText": "body mass index"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
}
