use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_study_design_document_type_phase_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000998.json"),
        r##"{
  "Core": { "Id": "CORE-000998", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[documentVersionIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study design references the same document version more than once.",
    "Output Variables": ["name", "Duplicate documentVersionIds"]
  }
}"##,
    )
    .expect("write duplicate rule");
    fs::write(
            rules_dir.join("CORE-001004.json"),
            r##"{
  "Core": { "Id": "CORE-001004", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and instanceType != \"ObservationalStudyDesign\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational study must use ObservationalStudyDesign.",
    "Output Variables": ["name", "studyType"]
  }
}"##,
        )
        .expect("write type rule");
    fs::write(
            rules_dir.join("CORE-001005.json"),
            r##"{
  "Core": { "Id": "CORE-001005", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and studyPhase.standardCode.code != \"C48660\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational phase must be Not Applicable.",
    "Output Variables": ["name", "studyType", "studyPhase"]
  }
}"##,
        )
        .expect("write phase rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Duplicate doc",
            "instanceType": "InterventionalStudyDesign",
            "documentVersionIds": ["DocV_1", "DocV_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Wrong class",
            "instanceType": "InterventionalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            }
          },
          {
            "id": "StudyDesign_3",
            "name": "Wrong phase",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C129000",
              "decode": "Patient Registry Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C15602",
                "decode": "Phase III Trial"
              }
            }
          },
          {
            "id": "StudyDesign_4",
            "name": "Valid observational",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C48660",
                "decode": "Not Applicable"
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

    let duplicate = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000998.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run duplicate");
    assert_eq!(
        duplicate.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(duplicate.results[0].error_count, 1);
    assert_eq!(
        duplicate.results[0].errors[0].variables,
        vec!["name", "Duplicate documentVersionIds"]
    );

    let type_rule = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-001004.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run type");
    assert_eq!(
        type_rule.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(type_rule.results[0].error_count, 1);
    assert_eq!(
        type_rule.results[0].errors[0].variables,
        vec!["name", "studyType"]
    );

    let phase = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-001005.json")],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run phase");
    assert_eq!(phase.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(phase.results[0].error_count, 1);
    assert_eq!(
        phase.results[0].errors[0].variables,
        vec!["name", "studyType", "studyPhase"]
    );
}

#[test]
fn run_validation_executes_usdm_study_design_duplicate_code_list_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            ("CORE-000980", "[\"name\", \"characteristics\"]"),
            ("CORE-001002", "[\"name\", \"subTypes\"]"),
            (
                "CORE-001003",
                "[\"name\", \"therapeuticAreas.codeSystem\", \"therapeuticAreas.codeSystemVersion\", \"therapeuticAreas\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Duplicate study design list values.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C1", "decode": "A" },
              { "id": "Code_2", "code": "C1", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_3", "code": "S1", "decode": "Sub A" },
              { "id": "Code_4", "code": "S1", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_5", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA A" },
              { "id": "Code_6", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA B" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Valid",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_7", "code": "C2", "decode": "A" },
              { "id": "Code_8", "code": "C3", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_9", "code": "S2", "decode": "Sub A" },
              { "id": "Code_10", "code": "S3", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_11", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T2", "decode": "TA A" },
              { "id": "Code_12", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T3", "decode": "TA B" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    for (id, variables) in [
        ("CORE-000980", vec!["name", "characteristics"]),
        ("CORE-001002", vec!["name", "subTypes"]),
        (
            "CORE-001003",
            vec![
                "name",
                "therapeuticAreas.codeSystem",
                "therapeuticAreas.codeSystemVersion",
                "therapeuticAreas",
            ],
        ),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run duplicate list");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_study_design_single_and_multi_centre_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001017.json"),
            r#"{
  "Core": { "Id": "CORE-001017", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[\"C217004\" in characteristics.code and \"C217005\" in characteristics.code].{\"check\": true}",
  "Outcome": {
    "Message": "A study design must not be both single-centre and multicentre.",
    "Output Variables": ["name", "characteristics"]
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
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Conflicting",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C217004", "decode": "Single-Centre" },
              { "id": "Code_2", "code": "C217005", "decode": "Multicentre" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Single only",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_3", "code": "C217004", "decode": "Single-Centre" }
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

    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "characteristics"]
    );
}
