use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};
#[test]
fn run_validation_executes_domain_presence_rule_against_loaded_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "DOMAIN": ["AE"] }
    },
    {
      "filename": "tt.csv",
      "domain": "TT",
      "records": { "DOMAIN": ["TT"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DOMAIN-PRESENCE.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-PRESENCE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Check": { "name": "TT", "operator": "exists" },
  "Outcome": { "Message": "TT dataset is present" }
}"#,
    )
    .expect("write domain presence rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "TT");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["TT"]);
}

#[test]
fn run_validation_executes_domain_presence_variable_exists_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pc.csv",
      "domain": "PC",
      "records": { "DOMAIN": ["PC"], "POOLID": ["POOL1"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DOMAIN-VARIABLE-EXISTS.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-VARIABLE-EXISTS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Operations": [{ "id": "$poolid_exists", "name": "POOLID", "operator": "variable_exists" }],
  "Check": {
    "all": [
      { "name": "$poolid_exists", "operator": "equal_to", "value": true },
      { "name": "POOLDEF", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "POOLID variable exists but POOLDEF dataset is missing",
    "Output Variables": ["$poolid_exists", "POOLDEF"]
  }
}"#,
    )
    .expect("write domain presence rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$poolid_exists", "POOLDEF"]
    );
}

#[test]
fn run_validation_reports_core_000677_poolid_values_missing_from_pooldef() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pooldef.csv",
      "domain": "POOLDEF",
      "records": {
        "POOLID": ["POOL1", "POOL2"]
      }
    },
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "DOMAIN": ["VS", "VS", "VS"],
        "POOLID": ["POOL1", "POOL3", ""],
        "VSSEQ": [1, 2, 3]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].dataset, "VS");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_passes_core_000677_when_pooldef_is_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "DOMAIN": ["VS"],
        "POOLID": ["POOL3"],
        "VSSEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_reports_core_000677_open_rules_data_dir_poolid_mismatches() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nvs,Vital Signs\npooldef,Pool Definition\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nvs,STUDYID,Study Identifier,Char,12\nvs,DOMAIN,Domain Abbreviation,Char,2\nvs,USUBJID,Unique Subject Identifier,Char,8\nvs,POOLID,Pool Identifier,Char,8\nvs,VSSEQ,Sequence Number,Num,8\npooldef,STUDYID,Study Identifier,Char,12\npooldef,POOLID,Pool Identifier,Char,8\npooldef,USUBJID,Unique Subject Identifier,Char,8\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("pooldef.csv"),
        "STUDYID,POOLID,USUBJID\nCDISCPILOT01,POOL1,\nCDISCPILOT01,POOL2,\n",
    )
    .expect("write pooldef csv");
    fs::write(
        data_dir.join("vs.csv"),
        "STUDYID,DOMAIN,USUBJID,POOLID,VSSEQ\nCDISCPILOT01,VS,SUBJ1,POOL4,1\nCDISCPILOT01,VS,SUBJ2,POOL2,2\n",
    )
    .expect("write vs csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.dataset, "VS");
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_reports_core_000896_poolid_values_missing_from_pooldef() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000896.yml"),
        r#"
Core:
  Id: CORE-000896
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: POOLDEF
    id: $pooldef_poolid
    name: POOLID
    operator: distinct
Check:
  all:
    - name: POOLID
      operator: non_empty
    - name: POOLID
      operator: is_not_contained_by
      value: $pooldef_poolid
Outcome:
  Message: POOLID value does not match a POOLID value in the POOLDEF dataset.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pooldef.csv",
      "domain": "POOLDEF",
      "records": {
        "POOLID": ["POOL1", "POOL2", "POOL3"]
      }
    },
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1", "S1", "S1", "S1"],
        "DOMAIN": ["VS", "VS", "VS", "VS", "VS", "VS", "VS"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3", "SUBJ4", "SUBJ5", "SUBJ6", "SUBJ7"],
        "POOLID": ["POOL1", "POOL1", "POOL1", "POOL3", "POOL2", "POOL2", "POOL99"],
        "VSSEQ": [1, 2, 3, 4, 6, 7, 8]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.dataset, "VS");
    assert_eq!(result.errors[0].row, Some(7));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_keeps_record_scoped_dataset_presence_when_oracle_expects_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ,AESTAT\n\
CDISC-TEST,AE,SUBJ1,1,NOT DONE\n\
CDISC-TEST,AE,SUBJ2,2,\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000013.json"),
        r#"{
  "Core": { "Id": "CORE-000013", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "AESTAT", "operator": "exists" },
  "Outcome": {
    "Message": "AESTAT variable is present in AE dataset.",
    "Output Variables": ["AESTAT"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("ae.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(
        result
            .errors
            .iter()
            .map(|issue| issue.row)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2)]
    );
}

#[test]
fn run_validation_collapses_dataset_level_presence_oracle_family() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ,AEOCCUR\n\
CDISC-TEST,AE,SUBJ1,1,Y\n\
CDISC-TEST,AE,SUBJ2,2,Y\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000012.json"),
        r#"{
  "Core": { "Id": "CORE-000012", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "AEOCCUR", "operator": "exists" },
  "Outcome": {
    "Message": "AEOCCUR is present in AE dataset.",
    "Output Variables": ["AEOCCUR"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("ae.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["AEOCCUR"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}
