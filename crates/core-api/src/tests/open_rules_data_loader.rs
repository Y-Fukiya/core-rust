use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

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
