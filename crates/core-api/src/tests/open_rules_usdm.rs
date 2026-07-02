use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_core_000878_reports_all_invalid_condition_context_ids() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000878.json"),
        r#"{
  "Core": { "Id": "CORE-000878", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ALL"], "Exclude": ["Activity", "ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "parent_rel", "operator": "equal_to", "value": "contextIds", "value_is_literal": true },
      { "name": "$condition_count", "operator": "non_empty" }
    ]
  },
  "Operations": [
    { "domain": "Condition", "filter": { "rel_type": "definition" }, "group": ["id", "instanceType"], "group_aliases": ["parent_id", "parent_entity"], "id": "$condition_count", "operator": "record_count" },
    { "domain": "Condition", "filter": { "rel_type": "definition" }, "group": ["id", "instanceType"], "group_aliases": ["parent_id", "parent_entity"], "id": "$condition_parent_entity", "name": "parent_entity", "operator": "distinct" }
  ],
  "Outcome": {
    "Message": "Invalid condition context.",
    "Output Variables": ["$condition_parent_entity", "$condition_parent_id", "$condition_parent_rel", "$condition_rel_type", "$condition_name", "id", "name", "parent_id", "parent_rel", "rel_type", "instanceType", "value", "$error_type"]
  }
}"#,
    )
    .expect("write condition context rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "condition.csv",
      "domain": "Condition",
      "records": {
        "parent_entity": ["StudyDesign", "Condition"],
        "parent_id": ["StudyDesign_1", "Condition_2"],
        "parent_rel": ["conditions", "contextIds"],
        "rel_type": ["definition", "reference"],
        "id": ["Condition_1", "Condition_1"],
        "name": ["COND1", "COND1"],
        "instanceType": ["Condition", "Condition"]
      }
    },
    {
      "filename": "biomedicalconcept.csv",
      "domain": "BiomedicalConcept",
      "records": {
        "parent_entity": ["Condition"],
        "parent_id": ["Condition_1"],
        "parent_rel": ["contextIds"],
        "rel_type": ["reference"],
        "id": ["BiomedicalConcept_1"],
        "name": ["Heart Rate"],
        "instanceType": ["BiomedicalConcept"]
      }
    },
    {
      "filename": "string.csv",
      "domain": "string",
      "records": {
        "parent_entity": ["Condition"],
        "parent_id": ["Condition_1"],
        "parent_rel": ["contextIds"],
        "rel_type": ["definition"],
        "value": ["Activity_missing"]
      }
    }
  ]
}"#,
    )
    .expect("write condition context data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let issue_count = outcome
        .results
        .iter()
        .flat_map(|result| result.errors.iter())
        .count();
    assert_eq!(issue_count, 3, "{:?}", outcome.results);
}

#[test]
fn run_validation_joins_usdm_match_dataset_before_unique_set() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-USDM-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-USDM-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Code"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Encounter",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Code" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Encounter" },
      { "name": "parent_rel", "operator": "equal_to", "value": "environmentalSetting", "value_is_literal": true },
      {
        "name": "code",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_rel", "parent_id", "codeSystem", "codeSystemVersion"]
      }
    ]
  },
  "Outcome": { "Message": "Duplicate environmental setting" }
}"#,
        )
        .expect("write USDM match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Encounter.csv",
      "domain": "Encounter",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["encounters"],
        "rel_type": ["definition"],
        "id": ["Encounter_1"],
        "name": ["E1"],
        "instanceType": ["Encounter"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["Encounter", "Encounter"],
        "parent_id": ["Encounter_1", "Encounter_1"],
        "parent_rel": ["environmentalSetting", "environmentalSetting"],
        "rel_type": ["definition", "definition"],
        "id": ["Code_84", "Code_85"],
        "code": ["C51282", "C51282"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "decode": ["Clinic", "Hospital"],
        "instanceType": ["Code", "Code"]
      }
    }
  ]
}"#,
    )
    .expect("write USDM match dataset data");

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
fn run_validation_executes_usdm_planned_enrollment_jsonata_unit_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000981.json"),
            r#"{
  "Core": { "Id": "CORE-000981", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InPopQ:=$boolean(plannedEnrollmentNumber.unit); {\"check\": $InPopQ=true} )][check = true]",
  "Outcome": {
    "Message": "A unit has been specified for a planned enrollment number",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
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
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "plannedEnrollmentNumber": {
                "id": "Quantity_1",
                "value": 22,
                "unit": {
                  "id": "Unit_1",
                  "standardCode": { "decode": "Day", "code": "C25301" }
                }
              },
              "cohorts": []
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
        "StudyDesignPopulation"
    );
}

#[test]
fn run_validation_executes_usdm_planned_enrollment_cohort_consistency_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000963.json"),
            r#"{
  "Core": { "Id": "CORE-000963", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InCohort:=$boolean(cohorts.plannedEnrollmentNumber); $InPop:=($type(plannedEnrollmentNumber) != \"null\" and $exists(plannedEnrollmentNumber)); {\"check\": (($InPop=true and $InCohort=true) or ($InPop=false and $InCohort=true))} )][check=true]",
  "Outcome": {
    "Message": "A planned enrollment number has been specified for both the study population and the cohorts, or it has been specified for only a subset of the cohorts.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
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
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_1",
                  "name": "COHORT1",
                  "plannedEnrollmentNumber": { "id": "Quantity_1", "value": 10 }
                },
                { "id": "StudyCohort_2", "name": "COHORT2" }
              ]
            }
          },
          {
            "id": "StudyDesign_2",
            "name": "Explicit null cohort quantity",
            "population": {
              "id": "Population_2",
              "name": "POP2",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_3",
                  "name": "COHORT3",
                  "plannedEnrollmentNumber": { "id": "Quantity_2", "value": 10 }
                },
                {
                  "id": "StudyCohort_4",
                  "name": "COHORT4",
                  "plannedEnrollmentNumber": null
                }
              ]
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
        "StudyDesignPopulation"
    );
}

#[test]
fn run_validation_executes_usdm_planned_completion_null_cohort_consistency_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000962.json"),
        r#"{
  "Core": { "Id": "CORE-000962", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InCohort:=$boolean(cohorts.plannedCompletionNumber); $InPop:=($type(plannedCompletionNumber) != \"null\" and $exists(plannedCompletionNumber)); {\"check\": (($InPop=true and $InCohort=true) or ($InPop=false and $InCohort=true))} )][check=true]",
  "Outcome": {
    "Message": "A planned completion number has been specified for both the study population and one or more cohorts, or it has been specified for only a subset of the cohorts.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedCompletionNumber.id",
      "plannedCompletionNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedCompletionNumber.id",
      "cohorts.plannedCompletionNumber(value/range)"
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
            "name": "Missing cohort quantity",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_1",
                  "name": "COHORT1",
                  "plannedCompletionNumber": { "id": "Quantity_1", "value": 10 }
                },
                { "id": "StudyCohort_2", "name": "COHORT2" }
              ]
            }
          },
          {
            "id": "StudyDesign_2",
            "name": "Explicit null cohort quantity",
            "population": {
              "id": "Population_2",
              "name": "POP2",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_3",
                  "name": "COHORT3",
                  "plannedCompletionNumber": { "id": "Quantity_2", "value": 10 }
                },
                {
                  "id": "StudyCohort_4",
                  "name": "COHORT4",
                  "plannedCompletionNumber": null
                }
              ]
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
}

#[test]
fn run_validation_executes_usdm_sponsor_role_applies_to_version_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000974.json"),
            r#"{
  "Core": { "Id": "CORE-000974", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "study.versions@$sv.($sv.roles[code.code = \"C70793\" and $not($sv.id in appliesToIds)])@$r.{\"check\": true}",
  "Outcome": {
    "Message": "The study role is a sponsor role (code.code is C70793) but it is not applicable to the study version.",
    "Output Variables": ["name", "code.code", "code.decode", "appliesToIds", "StudyVersion.id"]
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
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "ROLE_1",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": []
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyRole");
}

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

#[test]
fn run_validation_executes_usdm_governance_date_global_scope_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000968.json"),
            r#"{
  "Core": { "Id": "CORE-000968", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["GovernanceDate"] } },
  "Check": "study.documentedBy.versions.dateValues.{\"check\": true}",
  "Outcome": {
    "Message": "There is more than one date of this type for the study definition document version, but at least one of the dates has a global geographic scope.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "type",
      "dateValue",
      "geographicScopes.type"
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
            "version": "1",
            "dateValues": [
              {
                "id": "GovernanceDate_1",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-01",
                "geographicScopes": [
                  { "id": "GeographicScope_1", "type": { "code": "C68846", "decode": "Global" } }
                ]
              },
              {
                "id": "GovernanceDate_2",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-02",
                "geographicScopes": [
                  { "id": "GeographicScope_2", "type": { "code": "C41129", "decode": "Region" } }
                ]
              },
              {
                "id": "GovernanceDate_3",
                "instanceType": "GovernanceDate",
                "type": { "code": "C215663", "decode": "Effective Date" },
                "dateValue": "2020-01-03",
                "geographicScopes": [
                  { "id": "GeographicScope_3", "type": { "code": "C41129", "decode": "Region" } }
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "GovernanceDate");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_document_content_reference_one_to_one_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000985.json"),
            r#"{
  "Core": { "Id": "CORE-000985", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["DocumentContentReference"] } },
  "Check": "study.versions.amendments.changes.changedSections.{\"check\": true}",
  "Outcome": {
    "Message": "There is not a one-to-one relationship between the referenced section number and title within the study definition document affected by the study amendment.",
    "Output Variables": [
      "StudyAmendment.id",
      "StudyAmendment.name",
      "StudyChange.id",
      "StudyChange.name",
      "appliesToId",
      "sectionNumber",
      "sectionTitle"
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
      { "id": "StudyDefinitionDocument_1", "name": "Protocol" }
    ],
    "versions": [
      {
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "changes": [
              {
                "id": "StudyChange_1",
                "name": "Change 1",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_1",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "1",
                    "sectionTitle": "Intro"
                  },
                  {
                    "id": "DocumentContentReference_2",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "2",
                    "sectionTitle": "Intro"
                  }
                ]
              },
              {
                "id": "StudyChange_2",
                "name": "Change 2",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_3",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "3",
                    "sectionTitle": "Methods"
                  }
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "DocumentContentReference"
    );
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

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

#[test]
fn run_validation_executes_usdm_primary_endpoint_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(data_dir.join(".env"), "PRODUCT=USDM\nVERSION=4-0\n").expect("write env");

    fs::write(
            rules_dir.join("CORE-001036.json"),
            r##"{
  "Core": { "Id": "CORE-001036", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign", "InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Primary endpoints` = 0][]",
  "Outcome": {
    "Message": "There is not at least one endpoint with a level of primary within the study design.",
    "Output Variables": ["name", "# Primary endpoints"]
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
            "name": "Design without primary endpoint",
            "instanceType": "InterventionalStudyDesign",
            "objectives": [
              {
                "id": "Objective_1",
                "endpoints": [
                  { "id": "Endpoint_1", "level": { "code": "C98772", "decode": "Secondary" } }
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "# Primary endpoints"]
    );
}

#[test]
fn run_validation_executes_usdm_interventional_model_intervention_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001077.json"),
            r##"{
  "Core": { "Id": "CORE-001077", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The number of study interventions referenced for the interventional study design is not consistent with intervention model.",
    "Output Variables": ["name", "studyType.code", "studyType.decode", "model.code", "model.decode", "# Referenced Study Interventions"]
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
        "studyInterventions": [
          { "id": "StudyIntervention_1", "instanceType": "StudyIntervention" },
          { "id": "StudyIntervention_2", "instanceType": "StudyIntervention" }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Too few interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Enough interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1", "StudyIntervention_2"]
          },
          {
            "id": "StudyDesign_3",
            "name": "Single group",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82640", "decode": "Single Group" },
            "studyInterventionIds": ["StudyIntervention_1"]
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
            "studyType.code",
            "studyType.decode",
            "model.code",
            "model.decode",
            "# Referenced Study Interventions"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_duplicate_study_cell_arm_epoch_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000948.json"),
            r#"{
  "Core": { "Id": "CORE-000948", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Check": "$.study.versions.studyDesigns.studyCells.{\"check\": true}",
  "Outcome": {
    "Message": "The combination of arm and epoch occurs more than once within the study design.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "armId", "StudyArm.name", "epochId", "StudyEpoch.name"]
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
            "name": "Design 1",
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" }
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyCell");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDesign.id",
            "StudyDesign.name",
            "armId",
            "StudyArm.name",
            "epochId",
            "StudyEpoch.name"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_study_arm_missing_epoch_refs_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001026.json"),
        r#"{
  "Core": { "Id": "CORE-001026", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.arms@$sa.{\"check\": true}",
  "Outcome": {
    "Message": "The StudyArm does not have one StudyCell for each StudyEpoch.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "StudyDesign.epochs",
      "Arm's StudyCell Epoch Refs",
      "Missing Epoch Refs"
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
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A", "instanceType": "StudyArm" },
              { "id": "StudyArm_2", "name": "Arm B", "instanceType": "StudyArm" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Screening" },
              { "id": "StudyEpoch_2", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "armId": "StudyArm_1", "epochId": "StudyEpoch_2" },
              { "id": "StudyCell_3", "armId": "StudyArm_2", "epochId": "StudyEpoch_1" }
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyArm");
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_condition_applies_to_reference_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001038.json"),
        r#"{
  "Core": { "Id": "CORE-001038", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition"] } },
  "Check": "$.study.versions.**.conditions.appliesToIds.{\"check\": true}",
  "Outcome": {
    "Message": "Condition appliesToIds must reference an allowed instance type.",
    "Output Variables": ["name", "appliesTo id", "appliesTo instanceType"]
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
        "activities": [
          { "id": "Activity_1", "name": "Dose", "instanceType": "Activity" }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Bad condition",
            "instanceType": "Condition",
            "appliesToIds": ["Activity_1", "Missing_1", "Condition_1"]
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
    assert_eq!(outcome.results[0].errors[0].dataset, "Condition");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "appliesTo id", "appliesTo instanceType"]
    );
}

#[test]
fn run_validation_executes_usdm_parameter_map_reference_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001049.json"),
            r#"{
  "Core": { "Id": "CORE-001049", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ParameterMap"] } },
  "Check": "$.study.**.dictionaries.parameterMaps.{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the parameter map is not available elsewhere in the model.",
    "Output Variables": ["SyntaxTemplateDictionary.id", "SyntaxTemplateDictionary.name", "tag", "reference"]
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
        "activities": [
          { "id": "Activity_1", "name": "Dose", "label": "Dose activity", "instanceType": "Activity" }
        ],
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "Dictionary",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              {
                "id": "ParameterMap_1",
                "instanceType": "ParameterMap",
                "tag": "valid_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_1\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_2",
                "instanceType": "ParameterMap",
                "tag": "missing_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_xx\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_3",
                "instanceType": "ParameterMap",
                "tag": "partial_ref",
                "reference": "<usdm:ref attribute=\"label\" id=\"Activity_1\"></usdm:ref>"
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
    assert_eq!(outcome.results[0].errors[0].dataset, "ParameterMap");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "SyntaxTemplateDictionary.id",
            "SyntaxTemplateDictionary.name",
            "tag",
            "reference"
        ]
    );
}

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

#[test]
fn run_validation_executes_usdm_duplicate_document_version_ids_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001052.json"),
            r##"{
  "Core": { "Id": "CORE-001052", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Check": "$.study.versions.{\"check\": true}",
  "Outcome": {
    "Message": "The study version references the same study definition document version more than once.",
    "Output Variables": ["versionIdentifier", "Duplicate documentVersionIds"]
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
        "documentVersionIds": ["DocVersion_1", "DocVersion_2", "DocVersion_1"]
      },
      {
        "id": "StudyVersion_2",
        "versionIdentifier": "3",
        "documentVersionIds": ["DocVersion_1", "DocVersion_2"]
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
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyVersion");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["versionIdentifier", "Duplicate documentVersionIds"]
    );
}

#[test]
fn run_validation_executes_usdm_tag_parameter_dictionary_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001074.json"),
            r##"{
  "Core": { "Id": "CORE-001074", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint", "EligibilityCriterion"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
            rules_dir.join("CORE-001037.json"),
            r##"{
  "Core": { "Id": "CORE-001037", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint", "EligibilityCriterion"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write CORE-001037 rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "IE_Dict",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              { "id": "ParameterMap_1", "instanceType": "ParameterMap", "tag": "valid_tag" }
            ]
          }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Missing dictionary",
            "instanceType": "Condition",
            "text": "Use <usdm:tag name=\"missing_dict\"/>"
          },
          {
            "id": "Condition_2",
            "name": "Invalid dictionary",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_xx",
            "text": "Use <usdm:tag name=\"bad_dict\"/>"
          },
          {
            "id": "Condition_3",
            "name": "Missing tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"not_in_dictionary\"></usdm:tag>"
          },
          {
            "id": "Condition_4",
            "name": "Valid tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"valid_tag\"/>"
          }
        ],
        "studyDesigns": [
          {
            "population": {
              "criteria": [
                {
                  "id": "EligibilityCriterion_1",
                  "name": "Criterion with missing dictionary",
                  "instanceType": "EligibilityCriterion",
                  "text": "Use <usdm:tag name=\"criterion_missing_dict\"/>"
                }
              ]
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

    assert_eq!(outcome.results.len(), 2);
    for id in ["CORE-001037", "CORE-001074"] {
        let result = outcome
            .results
            .iter()
            .find(|result| result.rule_id == id)
            .expect("result by id");
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 4);
        assert_eq!(result.errors[0].dataset, "SyntaxTemplateText");
        assert_eq!(
            result.errors[0].variables,
            vec![
                "name",
                "Parameter reference",
                "Parameter name",
                "dictionaryId",
                "SyntaxTemplateDictionary.name",
                "Issue"
            ]
        );
    }
}

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

#[test]
fn run_validation_executes_usdm_scheduled_instance_design_reference_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (rule_id, field, parent_field) in [
        (
            "CORE-000950",
            "epochId",
            "Referenced epoch's parent StudyDesign.id",
        ),
        (
            "CORE-001039",
            "encounterId",
            "Referenced encounter's parent StudyDesign.id",
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{rule_id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["ScheduledActivityInstance"] }} }},
  "Check": "$.study.versions.studyDesigns.scheduleTimelines.instances.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Scheduled instance references an object outside the design.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "{field}",
      "{parent_field}"
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
            "name": "Design 1",
            "epochs": [{ "id": "StudyEpoch_1", "name": "Epoch 1" }],
            "encounters": [{ "id": "Encounter_1", "name": "Encounter 1" }],
            "scheduleTimelines": []
          },
          {
            "id": "StudyDesign_2",
            "name": "Design 2",
            "epochs": [{ "id": "StudyEpoch_2", "name": "Epoch 2" }],
            "encounters": [{ "id": "Encounter_2", "name": "Encounter 2" }],
            "scheduleTimelines": [
              {
                "id": "ScheduleTimeline_1",
                "instances": [
                  {
                    "id": "ScheduledActivityInstance_1",
                    "name": "Bad epoch",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_1",
                    "encounterId": "Encounter_2"
                  },
                  {
                    "id": "ScheduledActivityInstance_2",
                    "name": "Bad encounter",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_1"
                  },
                  {
                    "id": "ScheduledActivityInstance_3",
                    "name": "Good refs",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_2"
                  }
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
    let epoch_result = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-000950")
        .expect("epoch result");
    let encounter_result = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-001039")
        .expect("encounter result");
    assert_eq!(epoch_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(epoch_result.error_count, 1);
    assert_eq!(epoch_result.errors[0].row, Some(1));
    assert_eq!(encounter_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(encounter_result.error_count, 1);
    assert_eq!(encounter_result.errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_study_role_assigned_persons_and_orgs_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000997.json"),
        r##"{
  "Core": { "Id": "CORE-000997", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles[assignedPersons and organizationIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study role references both assigned persons and organizations.",
    "Output Variables": ["name", "code", "assignedPersons", "organizationIds"]
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
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Sponsor",
            "instanceType": "Organization"
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Person only",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_1", "name": "AP1" }
            ]
          },
          {
            "id": "StudyRole_2",
            "name": "Org only",
            "instanceType": "StudyRole",
            "code": { "code": "C215670", "decode": "Local Sponsor" },
            "organizationIds": ["Organization_1"]
          },
          {
            "id": "StudyRole_3",
            "name": "Both",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_2", "name": "AP2" }
            ],
            "organizationIds": ["Organization_1"]
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
        outcome.results[0].errors[0].variables,
        vec!["name", "code", "assignedPersons", "organizationIds"]
    );
}

#[test]
fn run_validation_executes_usdm_duration_quantity_text_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000994.json"),
        r##"{
  "Core": { "Id": "CORE-000994", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and not(text) and not(quantity)].{\"check\": true}",
  "Outcome": {
    "Message": "The quantity and text are both missing.",
    "Output Variables": ["text", "quantity"]
  }
}"##,
    )
    .expect("write rule");
    fs::write(
            rules_dir.join("CORE-000995.json"),
            r##"{
  "Core": { "Id": "CORE-000995", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and ((durationWillVary=true and quantity) or (durationWillVary=false and not(quantity)))].{\"check\": true}",
  "Outcome": {
    "Message": "The duration quantity conflicts with durationWillVary.",
    "Output Variables": ["quantity(value/range)", "durationWillVary"]
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
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "plannedDuration": {
                  "id": "Duration_1",
                  "instanceType": "Duration",
                  "durationWillVary": false
                }
              }
            ]
          }
        ],
        "studyInterventions": [
          {
            "id": "Intervention_1",
            "administrations": [
              {
                "id": "Administration_1",
                "duration": {
                  "id": "Duration_2",
                  "instanceType": "Duration",
                  "durationWillVary": true,
                  "quantity": {
                    "value": 24,
                    "unit": {
                      "standardCode": {
                        "decode": "Week",
                        "code": "C29844"
                      }
                    }
                  }
                }
              },
              {
                "id": "Administration_2",
                "duration": {
                  "id": "Duration_3",
                  "instanceType": "Duration",
                  "text": "Variable"
                }
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

    let missing = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000994.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run missing");
    assert_eq!(missing.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(missing.results[0].error_count, 1);
    assert_eq!(
        missing.results[0].errors[0].variables,
        vec!["text", "quantity"]
    );

    let conflict = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000995.json")],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run conflict");
    assert_eq!(
        conflict.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(conflict.results[0].error_count, 2);
    assert_eq!(
        conflict.results[0].errors[0].variables,
        vec!["quantity(value/range)", "durationWillVary"]
    );
}

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

#[test]
fn run_validation_executes_usdm_range_and_person_name_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
        ("CORE-001009", "Range", "[\"minValue\", \"maxValue\"]"),
        ("CORE-001012", "Range", "[\"minValue\", \"maxValue\"]"),
        ("CORE-001014", "PersonName", "[\"familyName\", \"text\"]"),
    ] {
        fs::write(
            rules_dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM recursive entity rule.",
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
            "population": {
              "plannedAge": {
                "instanceType": "Range",
                "minValue": {
                  "value": 50,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                },
                "maxValue": {
                  "value": 20,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                }
              },
              "plannedCompletionNumber": {
                "instanceType": "Range",
                "minValue": { "value": 50 },
                "maxValue": {
                  "value": 100,
                  "unit": { "standardCode": { "decode": "Participant", "code": "C142710" } }
                }
              }
            }
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "assignedPersons": [
              {
                "id": "AssignedPerson_1",
                "personName": {
                  "instanceType": "PersonName"
                }
              },
              {
                "id": "AssignedPerson_2",
                "personName": {
                  "instanceType": "PersonName",
                  "familyName": "Smith"
                }
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

    for (id, count, variables) in [
        ("CORE-001009", 1, vec!["minValue", "maxValue"]),
        ("CORE-001012", 1, vec!["minValue", "maxValue"]),
        ("CORE-001014", 1, vec!["familyName", "text"]),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run recursive entity");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, count);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_simple_recursive_entity_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
            (
                "CORE-000971",
                "Address",
                "[\"Organization.id\", \"Organization.name\", \"text\", \"lines\", \"district\", \"city\", \"postalCode\", \"state\", \"country\"]",
            ),
            (
                "CORE-001011",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\"]",
            ),
            (
                "CORE-001021",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\"]",
            ),
            (
                "CORE-001006",
                "BiomedicalConcept",
                "[\"name\", \"label/synonym\", \"synonyms\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM simple recursive rule.",
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
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Org",
            "legalAddress": {
              "id": "Address_1",
              "instanceType": "Address"
            }
          }
        ],
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C48660", "decode": "Not Applicable" }
            }
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "Role_1",
            "name": "Manufacturer",
            "instanceType": "ProductOrganizationRole"
          }
        ],
        "biomedicalConcepts": [
          {
            "id": "BC_1",
            "name": "Sex",
            "label": "Sex",
            "instanceType": "BiomedicalConcept",
            "synonyms": ["Gender", "sex"]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    for (id, variables) in [
        (
            "CORE-000971",
            vec![
                "Organization.id",
                "Organization.name",
                "text",
                "lines",
                "district",
                "city",
                "postalCode",
                "state",
                "country",
            ],
        ),
        (
            "CORE-001011",
            vec!["StudyAmendment.id", "StudyAmendment.name", "code"],
        ),
        ("CORE-001021", vec!["name", "appliesToIds"]),
        ("CORE-001006", vec!["name", "label/synonym", "synonyms"]),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run simple recursive");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_product_role_invalid_target_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001022.json"),
        r#"{
  "Core": { "Id": "CORE-001022", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ProductOrganizationRole"] } },
  "Check": "$.study.versions.productOrganizationRoles.{\"check\": true}",
  "Outcome": {
    "Message": "At least one of the appliesTo specifications does not apply to medical devices or administrable products.",
    "Output Variables": ["name", "appliesToIds", "appliesTo name"]
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
        "administrableProducts": [
          { "id": "AdmProd_1", "name": "Product" }
        ],
        "productOrganizationRoles": [
          {
            "id": "Role_1",
            "name": "Mixed valid and invalid target",
            "instanceType": "ProductOrganizationRole",
            "appliesToIds": ["AdmProd_1", "Missing_1"]
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
        outcome.results[0].errors[0].variables,
        vec!["name", "appliesToIds", "appliesTo name"]
    );
}

#[test]
fn run_validation_executes_usdm_administration_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            (
                "CORE-000966",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value)\", \"route.id\", \"route\"]",
            ),
            (
                "CORE-000967",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"frequency.id\", \"frequency\"]",
            ),
            (
                "CORE-000969",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"administrableProductId\", \"medicalDeviceId\", \"MedicalDevice.name\", \"MedicalDevice.embeddedProductId\", \"AdministrableProduct.name\"]",
            ),
            (
                "CORE-000986",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"administrableProductId\", \"AdministrableProduct.name\", \"medicalDeviceId\", \"MedicalDevice.name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Administration"] }} }},
  "Check": "study.versions.studyInterventions.administrations.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Administration rule.",
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
        "administrableProducts": [
          { "id": "AdmProd_1", "name": "Product 1" }
        ],
        "medicalDevices": [
          { "id": "MedDev_1", "name": "Device 1", "embeddedProductId": "AdmProd_1" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "name": "Intervention",
            "administrations": [
              {
                "id": "Administration_1",
                "name": "Route only",
                "instanceType": "Administration",
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_2",
                "name": "Dose without frequency or product",
                "instanceType": "Administration",
                "dose": {
                  "id": "Quantity_1",
                  "value": 30,
                  "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                },
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_3",
                "name": "Duplicated product",
                "instanceType": "Administration",
                "administrableProductId": "AdmProd_1",
                "medicalDeviceId": "MedDev_1"
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

    for (id, expected_count) in [
        ("CORE-000966", 1),
        ("CORE-000967", 1),
        ("CORE-000969", 2),
        ("CORE-000986", 1),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run administration rule");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, expected_count);
        assert_eq!(outcome.results[0].errors[0].dataset, "Administration");
    }
}

#[test]
fn run_validation_executes_usdm_strength_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            (
                "CORE-001007",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.value\"]",
            ),
            (
                "CORE-001008",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.minValue\", \"numerator.maxValue\"]",
            ),
            (
                "CORE-001020",
                "[\"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"denominator.id\", \"denominator.value\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Strength"] }} }},
  "Check": "study.versions.administrableProducts.ingredients.substance.strengths.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Strength rule.",
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
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product 1",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Subst_1",
                  "name": "Substance 1",
                  "strengths": [
                    {
                      "id": "Strength_1",
                      "name": "Numerator value",
                      "instanceType": "Strength",
                      "numerator": { "id": "Quantity_1", "value": 10 }
                    },
                    {
                      "id": "Strength_2",
                      "name": "Numerator range",
                      "instanceType": "Strength",
                      "numerator": {
                        "minValue": { "id": "Quantity_2", "value": 50 },
                        "maxValue": {
                          "id": "Quantity_3",
                          "value": 100,
                          "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                        }
                      }
                    },
                    {
                      "id": "Strength_3",
                      "name": "Denominator",
                      "instanceType": "Strength",
                      "denominator": { "id": "Quantity_4", "value": 2 }
                    }
                  ]
                }
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

    for id in ["CORE-001007", "CORE-001008", "CORE-001020"] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run strength rule");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "Strength");
    }
}

#[test]
fn run_validation_executes_usdm_embedded_product_sourcing_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001001.json"),
            r#"{
  "Core": { "Id": "CORE-001001", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["AdministrableProduct"] } },
  "Check": "(study.versions)@$sv.$sv.administrableProducts@$ap.[{\"check\": true}]",
  "Outcome": {
    "Message": "The sourcing is defined while the administrable product is only referenced to as an embedded product for a medical device.",
    "Output Variables": [
      "name",
      "sourcing",
      "MedicalDevice.id",
      "MedicalDevice.name",
      "MedicalDevice.embeddedProductId"
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
        "administrableProducts": [
          {
            "id": "AdministrableProduct_1",
            "name": "Embedded sourced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          },
          {
            "id": "AdministrableProduct_2",
            "name": "Embedded unsourced",
            "instanceType": "AdministrableProduct"
          },
          {
            "id": "AdministrableProduct_3",
            "name": "Admin referenced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          }
        ],
        "medicalDevices": [
          { "id": "MedicalDevice_1", "name": "Device", "embeddedProductId": "AdministrableProduct_1" },
          { "id": "MedicalDevice_2", "name": "Other Device", "embeddedProductId": "AdministrableProduct_2" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "administrations": [
              { "id": "Administration_1", "administrableProductId": "AdministrableProduct_3" }
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
    assert_eq!(outcome.results[0].errors[0].dataset, "AdministrableProduct");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_usdm_reference_and_duplicate_jsonata_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
            (
                "CORE-000970",
                "StudyRole",
                "[\"name\", \"code\", \"appliesToIds\", \"StudyVersion.id\", \"StudyVersion.studyDesigns.id\"]",
            ),
            (
                "CORE-001022",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\", \"appliesTo name\"]",
            ),
            (
                "CORE-001024",
                "StudyDesign",
                "[\"name\", \"studyType\"]",
            ),
            (
                "CORE-001032",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001033",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001031",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\", \"primaryReason.code\"]",
            ),
            (
                "CORE-000999",
                "StudyDefinitionDocumentVersion",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"version\"]",
            ),
            (
                "CORE-000983",
                "Procedure",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"Activity.id\", \"Activity.name\", \"name\", \"studyInterventionId\", \"StudyIntervention.name\"]",
            ),
            (
                "CORE-000984",
                "SubjectEnrollment",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"name\", \"forGeographicScope\", \"forStudySiteId\", \"forStudyCohortId\"]",
            ),
            (
                "CORE-001010",
                "Substance",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Parent Substance.id\", \"Parent Substance.name\", \"name\", \"referenceSubstance.id\", \"referenceSubstance.name\"]",
            ),
            (
                "CORE-001018",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\"]",
            ),
            (
                "CORE-001019",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\", \"Used in\"]",
            ),
            (
                "CORE-001025",
                "BiospecimenRetention",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"isRetained\"]",
            ),
            (
                "CORE-001027",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"criterionItemId\"]",
            ),
            (
                "CORE-001028",
                "EligibilityCriterionItem",
                "[\"StudyVersion.id\", \"StudyVersion.versionIdentifier\", \"name\"]",
            ),
            (
                "CORE-001029",
                "StudyCohort",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.indications.id\", \"StudyDesignPopulation.id\", \"StudyDesignPopulation.name\", \"name\", \"Invalid indicationIds\"]",
            ),
            (
                "CORE-001030",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"name\", \"Invalid studyInterventionIds\", \"Invalid StudyIntervention.name\"]",
            ),
            (
                "CORE-001040",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"studyInterventionIds value\", \"Referenced intervention's parent StudyDesign.id\"]",
            ),
            (
                "CORE-001045",
                "StudyArm",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.population.id\", \"StudyDesign.population.cohorts.id\", \"name\", \"populationId\"]",
            ),
            (
                "CORE-001042",
                "GeographicScope",
                "[\"type.code\", \"type.decode\", \"code.standardCode.code\", \"code.standardCode.decode\"]",
            ),
            (
                "CORE-001051",
                "NarrativeContent",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"StudyDefinitionDocumentVersion.id\", \"StudyDefinitionDocumentVersion.version\", \"name\", \"sectionNumber\", \"sectionTitle\"]",
            ),
            (
                "CORE-001050",
                "NarrativeContent",
                "[\"StudyProtocolDocument.id\", \"StudyProtocolDocument.name\", \"StudyProtocolDocumentVersion.id\", \"StudyProtocolDocumentVersion.protocolVersion\", \"name\", \"sectionNumber\", \"sectionTitle\", \"Invalid Reference\"]",
            ),
            (
                "CORE-001023",
                "InterventionalStudyDesign",
                "[\"name\", \"intentTypes\"]",
            ),
            (
                "CORE-001046",
                "StudyDesign",
                "[\"id\", \"name\", \"interventionModel.code\", \"interventionModel.decode\", \"# Study Interventions\"]",
            ),
            (
                "CORE-001013",
                "USDMObject",
                "[\"name\"]",
            ),
            (
                "CORE-001015",
                "USDMObject",
                "[\"name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM reference rule.",
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
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Empty content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "1",
                "sectionTitle": "Overview"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Invalid ref content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "2",
                "sectionTitle": "Reference",
                "childIds": ["NarrativeContent_1"],
                "text": "<usdm:ref attribute=\"text\" id=\"MissingCriterion\" klass=\"EligibilityCriterion\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ],
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "1",
        "geographicScopes": [
          {
            "id": "GeographicScope_1",
            "name": "Global with code",
            "instanceType": "GeographicScope",
            "type": { "code": "C68846", "decode": "Global" },
            "code": { "standardCode": { "code": "US", "decode": "United States" } }
          }
        ],
        "duplicateObjects": [
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          },
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          }
        ],
        "studyInterventions": [
          { "id": "StudyIntervention_1", "name": "Valid intervention" },
          { "id": "StudyIntervention_2", "name": "Other intervention" }
        ],
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Substance_1",
                  "name": "Parent substance",
                  "referenceSubstance": {
                    "id": "Substance_2",
                    "name": "Reference substance",
                    "instanceType": "Substance",
                    "referenceSubstance": { "id": "Substance_3", "name": "Invalid nested reference" }
                  }
                }
              }
            ]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "ObservationalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "characteristics": [
              { "id": "Code_1", "code": "C217006", "decode": "Single Country" },
              { "id": "Code_2", "code": "C217007", "decode": "Multiple Countries" },
              { "id": "Code_3", "code": "C46079", "decode": "Randomized" },
              { "id": "Code_4", "code": "C25689", "decode": "Stratification" }
            ],
            "activities": [
              {
                "id": "Activity_1",
                "name": "Activity",
                "definedProcedures": [
                  {
                    "id": "Procedure_1",
                    "name": "Procedure",
                    "instanceType": "Procedure",
                    "studyInterventionId": "StudyIntervention_2"
                  }
                ]
              }
            ],
            "population": {
              "id": "Population_1",
              "name": "Population",
              "criterionIds": ["EligibilityCriterion_1"],
              "cohorts": [
                {
                  "id": "Cohort_1",
                  "name": "Cohort",
                  "criterionIds": ["EligibilityCriterion_1"],
                  "indicationIds": ["Indication_bad"]
                }
              ]
            },
            "indications": [{ "id": "Indication_1", "name": "Indication" }],
            "eligibilityCriteria": [
              {
                "id": "EligibilityCriterion_1",
                "name": "Criterion 1",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "01"
              },
              {
                "id": "EligibilityCriterion_2",
                "name": "Criterion 2",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "02"
              }
            ],
            "biospecimenRetentions": [
              {
                "id": "BiospecimenRetention_1",
                "name": "Retention",
                "instanceType": "BiospecimenRetention",
                "isRetained": true
              }
            ],
            "elements": [
              {
                "id": "StudyElement_1",
                "name": "Element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_2"]
              }
            ],
            "arms": [
              {
                "id": "StudyArm_1",
                "name": "Arm",
                "instanceType": "StudyArm",
                "populationIds": ["Population_bad", "Population_worse"]
              }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Intent design",
            "instanceType": "InterventionalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "interventionModel": { "code": "C82640", "decode": "Single Group Design" },
            "studyInterventions": [
              { "id": "StudyDesignIntervention_1", "name": "Embedded intervention 1" },
              { "id": "StudyDesignIntervention_2", "name": "Embedded intervention 2" }
            ],
            "elements": [
              {
                "id": "StudyElement_2",
                "name": "Cross-design element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_1"]
              }
            ],
            "intentTypes": [
              { "id": "IntentType_1", "code": "C123", "decode": "Intent" },
              { "id": "IntentType_2", "code": "C123", "decode": "Intent duplicate" },
              { "id": "IntentType_3", "code": "C456", "decode": "Other intent" },
              { "id": "IntentType_4", "code": "C456", "decode": "Other intent duplicate" }
            ]
          }
        ],
        "eligibilityCriterionItems": [
          {
            "id": "EligibilityCriterionItem_unused",
            "name": "Unused criterion item",
            "instanceType": "EligibilityCriterionItem"
          }
        ],
        "roles": [
          {
            "id": "Role_1",
            "name": "Invalid role scope",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1", "StudyDesign_1"]
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "ProductRole_1",
            "name": "Invalid product role",
            "instanceType": "ProductOrganizationRole",
            "appliesToIds": ["StudyVersion_1"]
          }
        ],
        "amendments": [
          {
            "id": "Amendment_1",
            "name": "Amendment",
            "enrollments": [
              {
                "id": "Enrollment_1",
                "name": "Enrollment",
                "instanceType": "SubjectEnrollment"
              }
            ],
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C17649", "decode": "Other" }
            },
            "secondaryReasons": [
              {
                "id": "Reason_2",
                "instanceType": "StudyAmendmentReason",
                "code": { "code": "C17649", "decode": "Other" }
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

    for (id, dataset, expected_count) in [
        ("CORE-000970", "StudyRole", 1),
        ("CORE-001022", "ProductOrganizationRole", 1),
        ("CORE-001024", "StudyDesign", 1),
        ("CORE-001032", "StudyDesign", 1),
        ("CORE-001033", "StudyDesign", 1),
        ("CORE-001031", "StudyAmendmentReason", 1),
        ("CORE-000999", "StudyDefinitionDocumentVersion", 1),
        ("CORE-000983", "Procedure", 1),
        ("CORE-000984", "SubjectEnrollment", 1),
        ("CORE-001010", "Substance", 1),
        ("CORE-001018", "EligibilityCriterion", 1),
        ("CORE-001019", "EligibilityCriterion", 1),
        ("CORE-001025", "BiospecimenRetention", 1),
        ("CORE-001027", "EligibilityCriterion", 2),
        ("CORE-001028", "EligibilityCriterionItem", 1),
        ("CORE-001029", "StudyCohort", 1),
        ("CORE-001030", "StudyElement", 1),
        ("CORE-001040", "StudyElement", 2),
        ("CORE-001045", "StudyArm", 2),
        ("CORE-001042", "GeographicScope", 1),
        ("CORE-001051", "NarrativeContent", 1),
        ("CORE-001050", "NarrativeContent", 1),
        ("CORE-001023", "InterventionalStudyDesign", 2),
        ("CORE-001046", "StudyDesign", 1),
        ("CORE-001013", "USDMObject", 2),
        ("CORE-001015", "USDMObject", 2),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run reference rule");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{id}"
        );
        assert_eq!(outcome.results[0].error_count, expected_count, "{id}");
        assert_eq!(outcome.results[0].errors[0].dataset, dataset, "{id}");
    }
}

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
