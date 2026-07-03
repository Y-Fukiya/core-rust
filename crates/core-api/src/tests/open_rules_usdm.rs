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
