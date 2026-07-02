use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_activity_child_id_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001062.json"),
            r##"{
  "Core": { "Id": "CORE-001062", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities.childIds[$not($ in $.study.versions.studyDesigns.activities.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity references a childId that does not match the id of any activity defined within the same study design as the activity.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "name", "childId"]
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
            "name": "Design 1",
            "instanceType": "InterventionalStudyDesign",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Missing_A", "Missing_B"]
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "childIds": []
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
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["StudyDesign.id", "StudyDesign.name", "name", "childId"]
    );
}

#[test]
fn run_validation_executes_usdm_activity_children_with_details_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000954.json"),
            r##"{
  "Core": { "Id": "CORE-000954", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities[childIds and (biomedicalConceptIds or bcCategoryIds or definedProcedures or timelineId or bcSurrogateIds)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity has children but also refers to details.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "childIds",
      "biomedicalConceptIds",
      "bcCategoryIds",
      "bcSurrogateIds",
      "timelineId",
      "definedProcedures.id"
    ]
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
            "name": "Design 1",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent with timeline",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Activity_3"],
                "timelineId": "Timeline_1"
              },
              {
                "id": "Activity_2",
                "name": "Parent with BC",
                "instanceType": "Activity",
                "childIds": ["Activity_3"],
                "biomedicalConceptIds": ["BC_1"]
              },
              {
                "id": "Activity_3",
                "name": "Leaf with details",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BC_2"]
              },
              {
                "id": "Activity_4",
                "name": "Parent only",
                "instanceType": "Activity",
                "childIds": ["Activity_3"]
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
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDesign.id",
            "StudyDesign.name",
            "name",
            "childIds",
            "biomedicalConceptIds",
            "bcCategoryIds",
            "bcSurrogateIds",
            "timelineId",
            "definedProcedures.id"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_activity_child_order_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001066.json"),
            r#"{
  "Core": { "Id": "CORE-001066", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The previous/next ordering of the activity with respect to child activities is incorrect.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "previousId",
      "nextId",
      "childIds",
      "Parent Activity's id"
    ]
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
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2"],
                "nextId": "Activity_3"
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "previousId": "Activity_1"
              },
              {
                "id": "Activity_3",
                "name": "Other",
                "instanceType": "Activity",
                "previousId": "Activity_2"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_usdm_activity_bc_category_overlap_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001047.json"),
            r#"{
  "Core": { "Id": "CORE-001047", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions@$sv.$sv.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The activity references both a biomedical concept category and a biomedical concept, but the biomedical concept is a member of the referenced category or one of its subcategories.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "biomedicalConceptId",
      "bcCategoryId(s) containing BC"
    ]
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
        "bcCategories": [
          {
            "id": "BCCategory_1",
            "name": "Vitals",
            "memberIds": ["BiomedicalConcept_1"]
          },
          {
            "id": "BCCategory_2",
            "name": "Labs",
            "memberIds": ["BiomedicalConcept_2"]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Overlapping Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_1"]
              },
              {
                "id": "Activity_2",
                "name": "Non-overlap Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_2"]
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
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}
