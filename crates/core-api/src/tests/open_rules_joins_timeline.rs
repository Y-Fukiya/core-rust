use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};
#[test]
fn run_validation_joins_schedule_timeline_for_activity_epoch_presence() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["SCREEN1", "AE"],
        "epochId": ["", ""],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimeline.csv",
      "domain": "ScheduleTimeline",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "mainTimeline": [true, false],
        "instanceType": ["ScheduleTimeline", "ScheduleTimeline"]
      }
    }
  ]
}"#,
    )
    .expect("write schedule timeline match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_schedule_timeline_from_open_rules_csv() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

    fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\nScheduleTimeline,ScheduleTimeline,Schedule Timeline\n",
        )
        .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Identifier,String,[1]\nScheduledActivityInstance,name,Name,String,[1]\nScheduledActivityInstance,epochId,Epoch Identifier,String,StudyEpoch[0..1].id[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\nScheduleTimeline,parent_entity,Parent Entity Name,String,[1]\nScheduleTimeline,parent_id,Parent Entity Id,String,[1]\nScheduleTimeline,rel_type,Type of Relationship,String,[1]\nScheduleTimeline,id,Identifier,String,[1]\nScheduleTimeline,mainTimeline,Main Timeline Indicator,Boolean,[1]\nScheduleTimeline,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,epochId,instanceType\nScheduleTimeline,ScheduleTimeline_1,instances,definition,ScheduledActivityInstance_1,SCREEN1,,ScheduledActivityInstance\nScheduleTimeline,ScheduleTimeline_2,instances,definition,ScheduledActivityInstance_2,AE,,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");
    fs::write(
            data_dir.join("ScheduleTimeline.csv"),
            "parent_entity,parent_id,rel_type,id,mainTimeline,instanceType\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_1,True,ScheduleTimeline\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_2,False,ScheduleTimeline\n",
        )
        .expect("write schedule timeline csv");

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
}
