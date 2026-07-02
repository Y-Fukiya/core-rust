use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_main_timeline_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000407.json"),
        r##"{
  "Core": { "Id": "CORE-000407", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign", "InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Main timelines` != 1][]",
  "Outcome": {
    "Message": "The study design does not have exactly one main timeline.",
    "Output Variables": ["name", "# Main timelines", "Main timelines"]
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
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Study Design 1",
            "instanceType": "InterventionalStudyDesign",
            "scheduleTimelines": []
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
}

#[test]
fn run_validation_executes_usdm_main_timeline_planned_duration_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001016.json"),
        r##"{
  "Core": { "Id": "CORE-001016", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ScheduleTimeline"] } },
  "Check": "$.study.versions.studyDesigns.scheduleTimelines.{\"check\": true}",
  "Outcome": {
    "Message": "The planned duration is not specified for the main timeline.",
    "Output Variables": ["name", "mainTimeline"]
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
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "name": "Main Timeline",
                "mainTimeline": true
              },
              {
                "id": "Timeline_2",
                "name": "Auxiliary Timeline",
                "mainTimeline": false
              }
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
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "ScheduleTimeline");
}

#[test]
fn run_validation_executes_usdm_timeline_order_consistency_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(data_dir.join(".env"), "PRODUCT=USDM\nVERSION=4-0\n").expect("write env");

    for (rule_id, previous_next, timeline_refs) in [
        (
            "CORE-000961",
            "Encounter order by previous/next",
            "Encounter order by timeline refs",
        ),
        (
            "CORE-001048",
            "Epoch order by previous/next",
            "Epoch order by timeline refs",
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{rule_id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["StudyDesign", "InterventionalStudyDesign", "ObservationalStudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Timeline order is inconsistent.",
    "Output Variables": [
      "name",
      "ScheduleTimeline.id",
      "ScheduleTimeline.name",
      "ScheduleTimeline.mainTimeline",
      "{previous_next}",
      "{timeline_refs}"
    ]
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
            "epochs": [
              { "id": "Epoch_1", "name": "Screening", "nextId": "Epoch_2" },
              { "id": "Epoch_2", "name": "Treatment", "previousId": "Epoch_1", "nextId": "Epoch_3" },
              { "id": "Epoch_3", "name": "Follow-up", "previousId": "Epoch_2" }
            ],
            "encounters": [
              { "id": "Encounter_1", "name": "E1", "nextId": "Encounter_2" },
              { "id": "Encounter_2", "name": "E2", "previousId": "Encounter_1", "nextId": "Encounter_3" },
              { "id": "Encounter_3", "name": "E3", "previousId": "Encounter_2" }
            ],
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "name": "Main",
                "mainTimeline": true,
                "instances": [
                  { "id": "Instance_1", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_1", "encounterId": "Encounter_1" },
                  { "id": "Instance_2", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_3", "encounterId": "Encounter_3" },
                  { "id": "Instance_3", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_2", "encounterId": "Encounter_2" }
                ]
              }
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

    assert_eq!(outcome.results.len(), 2);
    for result in &outcome.results {
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].dataset, "StudyDesign");
        assert_eq!(result.errors[0].row, Some(1));
    }
}
