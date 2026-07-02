use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::tests::helpers::{write_rule, write_test_xpt_char_dataset};
use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_loads_xpt_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_rule(&rules_dir, "CORE-XPT-0001", "AE");
    let dataset_path = data_dir.join("ae.xpt");
    write_test_xpt_char_dataset(
        &dataset_path,
        "AE",
        &["STUDYID", "DOMAIN", "AESEQ"],
        &[vec!["CDISC-TEST", "AE", "1"], vec!["CDISC-TEST", "CM", "2"]],
    );

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_treats_safe_open_rules_missing_columns_as_null() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000200.json"),
        r#"{
  "Core": { "Id": "CORE-000200", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--STAT", "operator": "empty" },
          { "name": "--DRVFL", "operator": "not_equal_to", "value": "Y", "value_is_literal": true },
          { "name": "--ORRES", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "--ORRES cannot be null" }
}"#,
    )
    .expect("write open rules missing-column rule");
    let dataset_path = data_dir.join("lb.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1"],
        "LBSEQ": [1],
        "LBSTAT": [""],
        "LBORRES": ["12"]
      }
    }
  ]
}"#,
    )
    .expect("write open rules data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_treats_safe_usdm_missing_nested_columns_as_null() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000680.json"),
        r#"{
  "Core": { "Id": "CORE-000680", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Range"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Range" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel", "operator": "is_contained_by", "value": ["plannedCompletionNumber"] },
      {
        "not": {
          "any": [
            { "name": "unit", "operator": "equal_to", "value": false },
            { "name": "unit", "operator": "empty" },
            { "name": "unit", "operator": "not_exists" }
          ]
        }
      }
    ]
  },
  "Outcome": { "Message": "A unit is specified" }
}"#,
    )
    .expect("write usdm missing-column rule");
    let dataset_path = data_dir.join("range.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Range.csv",
      "domain": "Range",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["StudyDesignPopulation_2"],
        "parent_rel": ["plannedCompletionNumber"],
        "rel_type": ["definition"],
        "id": ["Range_6"],
        "instanceType": ["Range"]
      }
    }
  ]
}"#,
    )
    .expect("write usdm data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_skips_missing_column_oracle_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000017.json"),
        r#"{
  "Core": { "Id": "CORE-000017", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "POOLID",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "USUBJID has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write rule");
    fs::write(
        rules_dir.join("CORE-000092.json"),
        r#"{
  "Core": { "Id": "CORE-000092", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECSTAT",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "ECSTAT has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write second rule");
    fs::write(
        rules_dir.join("CORE-000016.json"),
        r#"{
  "Core": { "Id": "CORE-000016", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECMOOD",
    "operator": "empty"
  },
  "Outcome": { "Message": "ECMOOD has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write empty missing-column rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": [""]
      }
    },
    {
      "filename": "ec.xpt",
      "domain": "EC",
      "records": {
        "USUBJID": ["S1"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 3);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Skipped));
    assert!(outcome
        .results
        .iter()
        .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
}

#[test]
fn run_validation_skips_core_000674_missing_placeholder_column_as_oracle_gap() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000674.json"),
        r#"{
  "Core": { "Id": "CORE-000674", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IQ"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--VALTRG", "operator": "matches_regex", "value": "^-?([1-9]\\d*|0)(\\.\\d+)?$" },
      { "name": "--VALMAX", "operator": "matches_regex", "value": "^.+$" },
      { "name": "--VALTRG", "operator": "greater_than", "value": "--VALMAX" }
    ]
  },
  "Outcome": { "Message": "--VALTRG must be <= --VALMAX" }
}"#,
    )
    .expect("write core 674 rule");

    let dataset_path = data_dir.join("iq.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "iq.csv",
      "domain": "IQ",
      "records": {
        "IQVALTRG": [1]
      }
    }
  ]
}"#,
    )
    .expect("write core 674 data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_uses_open_rules_data_loader_when_requested() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("create rules dir");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::write(
        rules_dir.join("CORE-OPEN-0001.yml"),
        r#"Core:
  Id: CORE-OPEN-0001
  Status: Published
Scope:
  Domains: {}
  Classes: {}
Sensitivity: Record
Rule Type: Record Data
Check:
  name: CMSEQ
  operator: less_than_or_equal_to
  value: 0
Outcome:
  Message: CMSEQ must be greater than zero
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\ncm,Concomitant Medications\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(data_dir.join("cm.csv"), "CMSEQ\n001\n").expect("write dataset csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_reports_csv_line_record_for_selected_open_rules_row_scope_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("create rules dir");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::write(
        rules_dir.join("CORE-000025.yml"),
        r#"Core:
  Id: CORE-000025
  Status: Published
Scope:
  Domains:
    Include:
      - IE
Sensitivity: Record
Rule Type: Record Data
Check:
  all:
    - name: IESTRESC
      operator: not_equal_to
      value: IEORRES
Outcome:
  Message: IESTRESC is not equal to IEORRES
  Output Variables:
    - IEORRES
    - IESTRESC
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nie,Inclusion Exclusion Criteria Not Met\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nIE,USUBJID,Unique Subject Identifier,Char,40\nIE,IESEQ,Sequence Number,Num,8\nIE,IEORRES,Original Result,Char,40\nIE,IESTRESC,Standardized Result,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("ie.csv"),
        "USUBJID,IESEQ,IEORRES,IESTRESC\nSUBJ1,1,Y,Yup\n",
    )
    .expect("write dataset csv");

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
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_reports_previous_record_for_selected_open_rules_row_scope_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("create rules dir");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::write(
        rules_dir.join("CORE-000223.yml"),
        r#"Core:
  Id: CORE-000223
  Status: Published
Scope:
  Domains:
    Include:
      - DM
Sensitivity: Record
Rule Type: Record Data
Check:
  all:
    - name: ACTARMCD
      operator: empty
    - name: ARMNRS
      operator: empty
Outcome:
  Message: ACTARMCD is empty, but ARMNRS is not completed.
  Output Variables:
    - ACTARMCD
    - ARMNRS
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\ndm,Demographics\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nDM,USUBJID,Unique Subject Identifier,Char,40\nDM,ACTARMCD,Actual Arm Code,Char,40\nDM,ARMNRS,Reason Arm and/or Actual Arm is Null,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("dm.csv"),
        "USUBJID,ACTARMCD,ARMNRS\nSUBJ1,ZAN_LOW,\nSUBJ2,,\n",
    )
    .expect("write dataset csv");

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
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}
