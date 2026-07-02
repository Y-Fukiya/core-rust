use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_narrative_content_ref_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001073.json"),
            r##"{
  "Core": { "Id": "CORE-001073", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContentItem"] } },
  "Check": "$.study.versions.narrativeContentItems[$contains(text,/usdm:ref/)].{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the narrative content item text is not available elsewhere in the model.",
    "Output Variables": ["name", "Invalid Reference"]
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
        "studyIdentifiers": [
          {
            "id": "StudyIdentifier_1",
            "name": "NCT identifier",
            "instanceType": "StudyIdentifier",
            "text": "NCT-001"
          }
        ],
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Missing klass",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_2",
            "name": "Missing target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_xx\" klass=\"StudyIdentifier\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_3",
            "name": "Valid target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\" klass=\"StudyIdentifier\"></usdm:ref>"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContentItem");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "Invalid Reference"]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_item_id_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000944.json"),
            r##"{
  "Core": { "Id": "CORE-000944", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[contentItemId and $not(contentItemId in $.study.versions.narrativeContentItems.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The reference to the narrative content item is not targeting a narrative content item that has been defined within the study.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "contentItemId",
      "sectionNumber"
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
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Defined",
            "instanceType": "NarrativeContentItem"
          }
        ]
      }
    ],
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Valid",
                "instanceType": "NarrativeContent",
                "contentItemId": "NarrativeContentItem_1",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Missing A",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_A",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Missing B",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_B",
                "sectionNumber": "3"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "contentItemId",
            "sectionNumber"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_peer_refs_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001055.json"),
            r##"{
  "Core": { "Id": "CORE-001055", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[previousId or nextId or childIds].{\"check\": true}",
  "Outcome": {
    "Message": "The narrative content references a previous, next or child id value that does not match the id of any narrative content defined within the same study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "Invalid previousId",
      "Invalid nextId",
      "Invalid childIds"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Bad next",
                "instanceType": "NarrativeContent",
                "nextId": "Missing_Next",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Good",
                "instanceType": "NarrativeContent",
                "previousId": "NarrativeContent_1",
                "nextId": "NarrativeContent_3",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Bad previous and child",
                "instanceType": "NarrativeContent",
                "previousId": "Missing_Previous",
                "childIds": ["NarrativeContent_2", "Missing_Child"],
                "sectionNumber": "3"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "sectionNumber",
            "Invalid previousId",
            "Invalid nextId",
            "Invalid childIds"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_display_section_number_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000964.json"),
            r##"{
  "Core": { "Id": "CORE-000964", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionNumber=true and (sectionNumber=null or sectionNumber=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section number is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionNumber",
      "sectionNumber"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
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
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "displaySectionNumber",
            "sectionNumber"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_duplicate_section_number_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001041.json"),
            r#"{
  "Core": { "Id": "CORE-001041", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy@$sdd.$sdd.versions@$sddv.($sddv.contents[displaySectionNumber=true and sectionNumber].{\"check\": true})",
  "Outcome": {
    "Message": "The displayed section number is not unique within the study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "displaySectionNumber"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Duplicate A",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Duplicate B",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden duplicate",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_4",
                "name": "Unique",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "2.1"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_narrative_content_display_section_title_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000965.json"),
            r##"{
  "Core": { "Id": "CORE-000965", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionTitle=true and (sectionTitle=null or sectionTitle=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section title is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionTitle",
      "sectionTitle"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": "Introduction"
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
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "displaySectionTitle",
            "sectionTitle"
        ]
    );
}
