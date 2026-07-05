use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};
#[test]
fn run_validation_suffixes_referenced_match_columns_without_left_conflict() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Planned age must be specified either in the study population or in all cohorts." }
}"#,
        )
        .expect("write suffix match column rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population without age column"],
        "instanceType": ["StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["Population_1"],
        "parent_rel": ["cohorts"],
        "rel_type": ["definition"],
        "id": ["Cohort_1"],
        "name": ["Cohort age"],
        "plannedAge": [true],
        "instanceType": ["StudyCohort"]
      }
    }
  ]
}"#,
    )
    .expect("write suffix match column data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_treats_missing_left_study_cohort_as_null_join_columns() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000815.json"),
        r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["id.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
    )
    .expect("write missing cohort rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population age"],
        "plannedAge": [true],
        "instanceType": ["StudyDesignPopulation"]
      }
    }
  ]
}"#,
    )
    .expect("write missing cohort data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_left_joins_study_cohort_for_population_planned_sex_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000875.json"),
            r#"{
  "Core": { "Id": "CORE-000875", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedSex", "operator": "not_exists" },
                  { "name": "plannedSex", "operator": "empty" },
                  { "name": "plannedSex", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedSex.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedSex.StudyCohort", "operator": "empty" },
                  { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedSex", "operator": "equal_to", "value": true },
              { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned sex must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedSex", "id.StudyCohort", "name.StudyCohort", "plannedSex.StudyCohort"]
  }
}"#,
        )
        .expect("write planned sex rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedSex": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort sex", "No cohort sex"],
        "plannedSex": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned sex data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_study_cohort_for_population_planned_age_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedAge", "id.StudyCohort", "name.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
        )
        .expect("write planned age rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedAge": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort age", "No cohort age"],
        "plannedAge": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned age data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_alias_code_to_standard_code_alias_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000828.json"),
            r#"{
  "Core": { "Id": "CORE-000828", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["AliasCode"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Code",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "AliasCode" },
      { "name": "parent_rel.Code", "operator": "equal_to", "value": "standardCodeAliases", "value_is_literal": true },
      { "name": "standardCode.codeSystem", "operator": "equal_to_case_insensitive", "value": "codeSystem" },
      { "name": "standardCode.codeSystemVersion", "operator": "equal_to_case_insensitive", "value": "codeSystemVersion" },
      {
        "any": [
          { "name": "standardCode.code", "operator": "equal_to_case_insensitive", "value": "code" },
          { "name": "standardCode.decode", "operator": "equal_to_case_insensitive", "value": "decode" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The standard code alias is the same as the standard code.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "standardCode.codeSystem", "standardCode.codeSystemVersion", "standardCode.code", "standardCode.decode", "codeSystem", "codeSystemVersion", "code", "decode"]
  }
}"#,
        )
        .expect("write alias code rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "AliasCode.csv",
      "domain": "AliasCode",
      "records": {
        "parent_entity": ["StudyVersion", "BiomedicalConceptProperty"],
        "parent_id": ["StudyVersion_1", "BiomedicalConceptProperty_1"],
        "parent_rel": ["studyPhase", "code"],
        "rel_type": ["definition", "definition"],
        "id": ["AliasCode_1", "AliasCode_2"],
        "instanceType": ["AliasCode", "AliasCode"],
        "standardCode.code": ["C15601", "C25208"],
        "standardCode.codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "standardCode.codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "standardCode.decode": ["Phase II Trial", "WEIGHT"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["AliasCode", "AliasCode", "AliasCode", "AliasCode"],
        "parent_id": ["AliasCode_1", "AliasCode_1", "AliasCode_2", "AliasCode_2"],
        "parent_rel": ["standardCode", "standardCodeAliases", "standardCode", "standardCodeAliases"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Code_1", "Code_2", "Code_3", "Code_4"],
        "code": ["C15601", "c15601", "C25208", "C99904x3"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15", "2023-12-15", "2023-12-15"],
        "decode": ["Phase II Trial", "Different label", "WEIGHT", "Weight"],
        "instanceType": ["Code", "Code", "Code", "Code"]
      }
    }
  ]
}"#,
        )
        .expect("write alias code data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_scheduled_activity_for_fixed_reference_timing_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Fixed reference timing must be related to a scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "type.code", "relativeFromScheduledInstanceId", "id.ScheduledActivityInstance"]
  }
}"#,
        )
        .expect("write fixed reference timing rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["timings", "timings", "timings"],
        "rel_type": ["definition", "definition", "definition"],
        "id": ["Timing_1", "Timing_2", "Timing_3"],
        "name": ["Missing from", "Bad from", "Good from"],
        "type.code": ["C201358", "C201358", "C201358"],
        "type.decode": ["Fixed Reference", "Fixed Reference", "Fixed Reference"],
        "relativeFromScheduledInstanceId": ["", "ScheduledDecisionInstance_1", "ScheduledActivityInstance_1"],
        "instanceType": ["Timing", "Timing", "Timing"]
      }
    },
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["Timing"],
        "parent_id": ["Timing_3"],
        "parent_rel": ["relativeFromScheduledInstanceId"],
        "rel_type": ["reference"],
        "id": ["ScheduledActivityInstance_1"],
        "name": ["Dose"],
        "instanceType": ["ScheduledActivityInstance"]
      }
    }
  ]
}"#,
        )
        .expect("write fixed reference timing data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_scheduled_activity_from_open_rules_csv() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Fixed reference timing must be related to a scheduled activity instance." }
}"#,
        )
        .expect("write fixed reference timing rule");

    fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nTiming,Timing,Timing\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\n",
        )
        .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nTiming,parent_entity,Parent Entity Name,String,[1]\nTiming,parent_id,Parent Entity Id,String,[1]\nTiming,parent_rel,Name of Relationship from Parent Entity,String,[1]\nTiming,rel_type,Type of Relationship,String,[1]\nTiming,id,Timing Id,String,[1]\nTiming,name,Timing Name,String,[1]\nTiming,type.code,Timing Type Code,String,[1]\nTiming,relativeFromScheduledInstanceId,Timing Relative From Scheduled Instance,String,ScheduledInstance[0..1].id[1]\nTiming,instanceType,Instance Type,String,[1]\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Scheduled Activity Instance Id,String,[1]\nScheduledActivityInstance,name,Scheduled Activity Instance Name,String,[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("Timing.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,type.code,relativeFromScheduledInstanceId,instanceType\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_1,Missing from,C201358,,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_2,Bad from,C201358,ScheduledDecisionInstance_1,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_3,Good from,C201358,ScheduledActivityInstance_1,Timing\n",
        )
        .expect("write timing csv");
    fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType\nTiming,Timing_3,relativeFromScheduledInstanceId,reference,ScheduledActivityInstance_1,Dose,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");

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
}

#[test]
fn run_validation_left_joins_objective_for_primary_endpoint_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000874.json"),
            r#"{
  "Core": { "Id": "CORE-000874", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Endpoint"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Objective",
      "Join Type": "left",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "level.code", "operator": "equal_to", "value": "C94496" },
      { "name": "level.code.Objective", "operator": "not_equal_to", "value": "C85826" }
    ]
  },
  "Outcome": {
    "Message": "The primary endpoint (level.code = C94496) is not referenced by a primary objective (level.code.Objective = C85826).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "level.code", "name.Objective", "level.code.Objective"]
  }
}"#,
        )
        .expect("write primary endpoint objective rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Endpoint.csv",
      "domain": "Endpoint",
      "records": {
        "parent_entity": ["Objective", "Objective"],
        "parent_id": ["Objective_1", "Objective_2"],
        "parent_rel": ["endpoints", "endpoints"],
        "rel_type": ["definition", "definition"],
        "id": ["Endpoint_1", "Endpoint_2"],
        "name": ["Primary bad", "Primary good"],
        "level.code": ["C94496", "C94496"],
        "level.decode": ["Primary", "Primary"],
        "instanceType": ["Endpoint", "Endpoint"]
      }
    },
    {
      "filename": "Objective.csv",
      "domain": "Objective",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["objectives", "objectives"],
        "rel_type": ["definition", "definition"],
        "id": ["Objective_1", "Objective_2"],
        "name": ["Secondary objective", "Primary objective"],
        "level.code": ["C85827", "C85826"],
        "level.decode": ["Secondary", "Primary"],
        "instanceType": ["Objective", "Objective"]
      }
    }
  ]
}"#,
    )
    .expect("write primary endpoint objective data");

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
fn run_validation_left_joins_study_epochs_for_study_arm_cell_coverage() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000797.json"),
            r#"{
  "Core": { "Id": "CORE-000797", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Join Type": "left",
      "Keys": ["parent_entity", "parent_id", "rel_type"]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyArm" },
      { "name": "id.StudyEpoch", "operator": "is_unique_set", "value": "id" },
      {
        "any": [
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
              { "name": "rel_type", "operator": "equal_to", "value": "definition" },
              { "name": "parent_rel", "operator": "equal_to", "value": "arms" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochs" }
            ]
          },
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyCell" },
              { "name": "rel_type", "operator": "equal_to", "value": "reference" },
              { "name": "parent_rel", "operator": "equal_to", "value": "armId" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochId" }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The StudyArm does not have a StudyCell for the StudyEpoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "id.StudyEpoch", "name.StudyEpoch"]
  }
}"#,
        )
        .expect("write study arm coverage rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["arms", "arms", "armId", "armId", "armId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyArm_1", "StudyArm_2", "StudyArm_1", "StudyArm_1", "StudyArm_2"],
        "name": ["Placebo", "Active", "Placebo", "Placebo", "Active"],
        "instanceType": ["StudyArm", "StudyArm", "StudyArm", "StudyArm", "StudyArm"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["epochs", "epochs", "epochId", "epochId", "epochId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1"],
        "name": ["Screening", "Treatment", "Screening", "Treatment", "Screening"],
        "instanceType": ["StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
        )
        .expect("write study arm coverage data");

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
fn run_validation_joins_activity_for_duplicate_biomedical_category_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000811.json"),
            r#"{
  "Core": { "Id": "CORE-000811", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConceptCategory"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Activity",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        { "Left": "parent_entity", "Right": "instanceType" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConceptCategory" },
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "rel_type.Activity", "operator": "equal_to", "value": "definition" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Activity" },
      { "name": "parent_rel", "operator": "equal_to", "value": "bcCategoryIds", "value_is_literal": true },
      {
        "name": "id",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_id", "parent_rel", "rel_type.Activity"]
      }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept category is referenced more than once from the same activity.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "name.Activity"]
  }
}"#,
        )
        .expect("write duplicate biomedical category rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "BiomedicalConceptCategory.csv",
      "domain": "BiomedicalConceptCategory",
      "records": {
        "parent_entity": ["Activity", "Activity", "Activity"],
        "parent_id": ["Activity_1", "Activity_1", "Activity_1"],
        "parent_rel": ["bcCategoryIds", "bcCategoryIds", "bcCategoryIds"],
        "rel_type": ["reference", "reference", "reference"],
        "id": ["BiomedicalConceptCategory_1", "BiomedicalConceptCategory_1", "BiomedicalConceptCategory_2"],
        "name": ["Vital Signs", "Vital Signs", "Labs"],
        "instanceType": ["BiomedicalConceptCategory", "BiomedicalConceptCategory", "BiomedicalConceptCategory"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["activities"],
        "rel_type": ["definition"],
        "id": ["Activity_1"],
        "name": ["Vital signs tests"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
        )
        .expect("write duplicate biomedical category data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_string_synonym_for_biomedical_concept_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000803.json"),
            r#"{
  "Core": { "Id": "CORE-000803", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConcept"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "string",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConcept" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel.string", "operator": "equal_to", "value": "synonyms", "value_is_literal": true },
      { "name": "value", "operator": "equal_to_case_insensitive", "value": "name" }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept synonym value is the same as the biomedical concept name (case insensitive).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "parent_rel.string", "value"]
  }
}"#,
        )
        .expect("write biomedical concept synonym rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "BiomedicalConcept.csv",
      "domain": "BiomedicalConcept",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["biomedicalConcepts", "biomedicalConcepts"],
        "rel_type": ["definition", "definition"],
        "id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "name": ["Race", "Weight"],
        "instanceType": ["BiomedicalConcept", "BiomedicalConcept"]
      }
    },
    {
      "filename": "string.csv",
      "domain": "string",
      "records": {
        "parent_entity": ["BiomedicalConcept", "BiomedicalConcept"],
        "parent_id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "parent_rel": ["synonyms", "synonyms"],
        "rel_type": ["definition", "definition"],
        "value": ["race", "Mass"]
      }
    }
  ]
}"#,
    )
    .expect("write biomedical concept synonym data");

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
fn run_validation_joins_timeline_exit_parent_for_scheduled_activity_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000819.json"),
            r#"{
  "Core": { "Id": "CORE-000819", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimelineExit",
      "Keys": [
        { "Left": "timelineExitId", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "ScheduledActivityInstance" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "timelineExitId", "operator": "non_empty" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.ScheduleTimelineExit" }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance references a timeline exit that is not defined within the same schedule timeline as the scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "timelineExitId", "parent_id.ScheduleTimelineExit"]
  }
}"#,
        )
        .expect("write timeline exit match rule");

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
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["OK", "BAD"],
        "timelineExitId": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimelineExit.csv",
      "domain": "ScheduleTimelineExit",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["exits", "exits"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduleTimelineExit", "ScheduleTimelineExit"]
      }
    }
  ]
}"#,
    )
    .expect("write timeline exit match data");

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
fn run_validation_joins_study_arm_parent_for_study_cell_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000835.json"),
            r#"{
  "Core": { "Id": "CORE-000835", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "armId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyArm" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an arm that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "armId", "parent_id.StudyArm"]
  }
}"#,
        )
        .expect("write study arm match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "armId": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["arms", "arms"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyArm", "StudyArm"]
      }
    }
  ]
}"#,
    )
    .expect("write study arm match data");

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
fn run_validation_joins_study_epoch_parent_for_study_cell_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000836.json"),
            r#"{
  "Core": { "Id": "CORE-000836", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Keys": [
        { "Left": "epochId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyEpoch" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an epoch that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "epochId", "parent_id.StudyEpoch"]
  }
}"#,
        )
        .expect("write study epoch match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "epochId": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["epochs", "epochs"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
    )
    .expect("write study epoch match data");

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
fn run_validation_joins_single_match_dataset_to_each_scoped_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MULTI-BASE-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-MULTI-BASE-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "RFSTDTC",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Reference start date must be reviewed" }
}"#,
    )
    .expect("write match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"],
        "DOMAIN": ["AE"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S2"],
        "DOMAIN": ["CM"],
        "CMSEQ": [1]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1", "S2"],
        "RFSTDTC": ["BAD", "OK"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let failed = outcome
        .results
        .iter()
        .find(|result| result.dataset == "AE")
        .expect("AE result");
    assert_eq!(failed.execution_status, ExecutionStatus::Failed);
    assert_eq!(failed.error_count, 1);
    let passed = outcome
        .results
        .iter()
        .find(|result| result.dataset == "CM")
        .expect("CM result");
    assert_eq!(passed.execution_status, ExecutionStatus::Passed);
}
