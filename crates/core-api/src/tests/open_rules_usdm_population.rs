use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

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
