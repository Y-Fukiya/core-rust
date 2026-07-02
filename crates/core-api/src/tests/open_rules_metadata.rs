use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
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

#[test]
fn run_validation_core_000638_reports_dataset_level_forbidden_send_variable() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("dm.csv"),
        "STUDYID,DOMAIN,USUBJID,DTHDTC\n\
CDISC-TEST,DM,SUBJ1,\n\
CDISC-TEST,DM,SUBJ2,\n\
CDISC-TEST,DM,SUBJ3,\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000638.json"),
        r#"{
  "Core": { "Id": "CORE-000638", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DTHDTC", "operator": "exists" },
  "Outcome": {
    "Message": "DTHDTC must not be present in SEND dataset",
    "Output Variables": ["DTHDTC"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("dm.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["DTHDTC"]);
    assert_eq!(result.errors[0].usubjid, None);
}

#[test]
fn run_validation_executes_dataset_metadata_record_count_rule() {
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
      "filename": "vs.csv",
      "domain": "VS",
      "label": "Vital Signs",
      "records": { "DOMAIN": [] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DATASET-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-DATASET-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$record_count", "operator": "record_count" }],
  "Check": { "name": "$record_count", "operator": "equal_to", "value": 0 },
  "Outcome": {
    "Message": "Dataset may not be empty",
    "Output Variables": ["dataset_name", "dataset_label", "$record_count"]
  }
}"#,
    )
    .expect("write metadata rule");

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
    assert_eq!(outcome.results[0].errors[0].dataset, "VS");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["dataset_name", "dataset_label", "$record_count"]
    );
}

#[test]
fn run_validation_executes_dataset_metadata_domain_prefix_rule() {
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
      "filename": "ab.csv",
      "domain": "LB",
      "label": "Laboratory Test Results A",
      "records": { "DOMAIN": ["LB"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DATASET-PREFIX.json"),
        r#"{
  "Core": { "Id": "CORE-DATASET-PREFIX", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Check": { "name": "dataset_name", "operator": "prefix_not_equal_to", "value": "DOMAIN" },
  "Outcome": {
    "Message": "Dataset name must begin with DOMAIN",
    "Output Variables": ["dataset_name"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "LB");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["dataset_name"]);
}

#[test]
fn run_validation_executes_dataset_metadata_dataset_names_rule() {
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
      "filename": "qs36.csv",
      "domain": "QS",
      "label": "Questionnaires SF-36",
      "records": { "DOMAIN": ["QS"] }
    },
    {
      "filename": "ae.csv",
      "domain": "AE",
      "label": "Adverse Events",
      "records": { "DOMAIN": ["AE"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-DATASET-NAMES.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-NAMES", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$list_dataset_names", "operator": "dataset_names" }],
  "Check": {
    "all": [
      { "name": "dataset_name", "operator": "matches_regex", "value": "^[A-Z]{2}[A-Z0-9]{1,2}" },
      { "name": "dataset_name", "operator": "not_prefix_matches_regex", "prefix": 2, "value": "(AP|FA)" },
      { "name": "dataset_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$list_dataset_names" }
    ]
  },
  "Outcome": {
    "Message": "Split dataset parent domain is missing",
    "Output Variables": ["dataset_name", "$list_dataset_names"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("failed dataset metadata result");
    assert_eq!(failed.error_count, 1);
    assert_eq!(failed.errors[0].dataset, "QS");
    assert_eq!(
        failed.errors[0].variables,
        vec!["dataset_name", "$list_dataset_names"]
    );
}

#[test]
fn run_validation_core_000540_treats_alphanumeric_fa_split_dataset_names_as_candidates() {
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
      "filename": "fa.csv",
      "domain": "FA",
      "label": "Findings About",
      "records": { "DOMAIN": ["FA"] }
    },
    {
      "filename": "facm.csv",
      "domain": "FACM",
      "label": "Findings About CM Records",
      "records": { "DOMAIN": ["FA"] }
    },
    {
      "filename": "fa1.csv",
      "domain": "FA1",
      "label": "Findings About Medical History",
      "records": { "DOMAIN": ["FA"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000540.json"),
        r#"{
  "Core": { "Id": "CORE-000540", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"], "include_split_datasets": true }, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$list_dataset_names", "operator": "dataset_names" }],
  "Check": {
    "all": [
      { "name": "dataset_name", "operator": "matches_regex", "value": "^[a-z(?-i)]{2}[a-z(?-i)]{1,2}" },
      { "name": "dataset_name", "operator": "prefix_equal_to", "prefix": 2, "value": "FA" },
      { "name": "dataset_name", "operator": "suffix_is_not_contained_by", "suffix": 2, "value": "$list_dataset_names" }
    ]
  },
  "Outcome": {
    "Message": "Parent domain referenced in Findings About dataset name is not present in the study",
    "Output Variables": ["dataset_name", "$list_dataset_names"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();

    assert_eq!(failed.len(), 2, "{failed:#?}");
    assert_eq!(failed[0].errors[0].dataset, "FACM");
    assert_eq!(failed[1].errors[0].dataset, "FA1");
}

#[test]
fn run_validation_executes_variable_metadata_label_length_rule() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "A label that is definitely longer than forty characters", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-VARIABLE-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Check": { "name": "variable_label", "operator": "longer_than", "value": 40 },
  "Outcome": {
    "Message": "Variable label is too long",
    "Output Variables": ["variable_label"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["variable_label"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_expected_variables_rule() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
        r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$dataset_variables", "$expected_variables"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_required_variables_rule() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-REQUIRED-VARIABLES.json"),
        r#"{
  "Core": { "Id": "CORE-REQUIRED-VARIABLES", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$required_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one required variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$required_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$dataset_variables", "$required_variables"]
    );
}

#[test]
fn run_validation_does_not_require_usubjid_for_trial_design_required_variables() {
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
      "filename": "ta.xpt",
      "domain": "TA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "ARMCD", "label": "Planned Arm Code", "type": "Char", "length": 20 },
        { "name": "ARM", "label": "Description of Planned Arm", "type": "Char", "length": 200 },
        { "name": "TAETORD", "label": "Planned Order of Element within Arm", "type": "Num", "length": 8 },
        { "name": "ETCD", "label": "Element Code", "type": "Char", "length": 8 },
        { "name": "ELEMENT", "label": "Description of Element", "type": "Char", "length": 200 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["TA"],
        "ARMCD": ["A"],
        "ARM": ["Active"],
        "TAETORD": [1],
        "ETCD": ["E1"],
        "ELEMENT": ["Treatment"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000355.json"),
        r#"{
  "Core": { "Id": "CORE-000355", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$required_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one required variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$required_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

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
fn run_validation_skips_core_000356_required_value_dataset_metadata_oracle_gap() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": [""],
        "DOMAIN": ["AE"],
        "USUBJID": ["01"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000356.json"),
        r#"{
  "Core": { "Id": "CORE-000356", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Dataset Metadata",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$required_variables", "operator": "exists" },
      { "name": "variable_name", "operator": "is_contained_by", "value": "$required_variables" },
      { "name": "variable_value", "operator": "empty" }
    ]
  },
  "Outcome": {
    "Message": "At least one Required variable has a null value",
    "Output Variables": ["variable_name", "variable_value"]
  }
}"#,
    )
    .expect("write value metadata rule");

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
    assert_eq!(outcome.results[0].error_count, 0);
    assert!(outcome.results[0].errors.is_empty());
}

#[test]
fn run_validation_passes_variable_metadata_expected_variables_when_all_are_present() {
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
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXDOSE", "label": "Dose", "type": "Num", "length": 8 },
        { "name": "EXDOSU", "label": "Dose Units", "type": "Char", "length": 20 },
        { "name": "EXDOSFRM", "label": "Dose Form", "type": "Char", "length": 20 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 },
        { "name": "EXENDTC", "label": "End Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXDOSE": ["10"],
        "EXDOSU": ["mg"],
        "EXDOSFRM": ["TABLET"],
        "EXSTDTC": ["2024-01-01"],
        "EXENDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
        r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

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
fn run_validation_executes_variable_metadata_timing_variables_rule() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000575.json"),
            r#"{
  "Core": { "Id": "CORE-000575", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" },
    { "id": "$timing_variables", "key_name": "role", "key_value": "Timing", "operator": "get_model_filtered_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$dataset_variables", "operator": "shares_no_elements_with", "value": "$timing_variables" }
    ]
  },
  "Outcome": {
    "Message": "No timing variable is provided",
    "Output Variables": ["$dataset_variables", "$timing_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .unwrap_or_else(|| panic!("AE timing failure: {:?}", outcome.results));
    assert_eq!(failed.dataset, "AE");
    assert_eq!(failed.error_count, 1);
    assert_eq!(failed.errors[0].row, None);
    assert_eq!(
        failed.errors[0].variables,
        vec!["$dataset_variables", "$timing_variables"]
    );
    assert!(
        !outcome
            .results
            .iter()
            .any(|result| result.dataset == "EX"
                && result.execution_status == ExecutionStatus::Failed)
    );
}

#[test]
fn run_validation_executes_variable_metadata_model_column_order_rule() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AESTDYXX", "label": "Custom Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"],
        "AESTDYXX": [1]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000550.json"),
            r#"{
  "Core": { "Id": "CORE-000550", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "Variables not listed in the Model List of Allowed Variables for Observation Class should be in SUPPQUAL.",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, Some(4));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["variable_name"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_library_column_order_rule() {
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
      "class": "EVENTS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "DOMAIN": ["AE"],
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000852.json"),
            r#"{
  "Core": { "Id": "CORE-000852", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$column_order_from_library", "operator": "get_column_order_from_library" },
    { "id": "$column_order_from_dataset", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "$column_order_from_dataset", "operator": "is_not_ordered_subset_of", "value": "$column_order_from_library" }
    ]
  },
  "Outcome": {
    "Message": "Variables are not in the correct order.",
    "Output Variables": ["$column_order_from_dataset", "$column_order_from_library"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$column_order_from_dataset", "$column_order_from_library"]
    );
}

#[test]
fn run_validation_executes_value_check_with_variable_metadata_rules() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AECAT", "label": "Category", "type": "Char", "length": 40 },
        { "name": "AESTDY", "label": "Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST", "CDISC-TEST"],
        "AETERM": ["HEADACHE", " NAUSEA"],
        "AECAT": [".", "GENERAL"],
        "AESTDY": [1, 2]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    for (id, check) in [
        (
            "CORE-000867",
            r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "matches_regex", "value": "^\\s" }
    ] }"#,
        ),
        (
            "CORE-000890",
            r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "non_empty" },
      { "name": "variable_value", "operator": "equal_to", "value": ".", "value_is_literal": true }
    ] }"#,
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{ "Include": ["ALL"] }}, "Classes": {{ "Include": ["ALL"] }} }},
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Variable Metadata",
  "Check": {check},
  "Outcome": {{
    "Message": "Value metadata rule.",
    "Output Variables": ["variable_value", "variable_name"]
  }}
}}"#
            ),
        )
        .expect("write rule");
    }

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().any(|result| {
        result.rule_id == "CORE-000867"
            && result.errors[0].row == Some(2)
            && result.errors[0].variables == vec!["variable_value", "variable_name"]
    }));
    assert!(failed.iter().any(|result| {
        result.rule_id == "CORE-000890"
            && result.errors[0].row == Some(1)
            && result.errors[0].variables == vec!["variable_value", "variable_name"]
    }));
}

#[test]
fn run_validation_executes_selected_library_metadata_rules() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AESDTH", "label": "Death", "type": "Char", "length": 1 }
      ],
      "records": { "STUDYID": ["S1"], "AESDTH": ["Y"] }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "XRACE", "label": "Race Extension", "type": "Char", "length": 20 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["DM"], "XRACE": ["BLUE"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Distinct Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Blabla", "type": "Char", "length": 8 },
        { "name": "VSORRESU", "label": "Original Units as Collected", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["VS"],
        "USUBJID": ["S1"],
        "VSSEQ": [1],
        "VSTESTCD": ["SYSBP"],
        "VSORRESU": ["mmHg"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
        rules_dir.join("CORE-000398.json"),
        r#"{
  "Core": { "Id": "CORE-000398", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "library_variable_label", "operator": "non_empty" },
    { "name": "variable_label", "operator": "not_equal_to", "value": "library_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable does not correspond to the label in the IG",
    "Output Variables": ["variable_name", "variable_label", "library_variable_label"]
  }
}"#,
    )
    .expect("write label rule");
    fs::write(
            rules_dir.join("CORE-000903.json"),
            r#"{
  "Core": { "Id": "CORE-000903", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "SE", "CO"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "exists" },
    { "name": "variable_name", "operator": "not_equal_to", "value": "library_variable_name" }
  ] },
  "Outcome": {
    "Message": "The variable is not allowed in this domain as it is not specified in the SENDIG for the specific domain",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write allowed-variable rule");
    fs::write(
        rules_dir.join("CORE-000507.json"),
        r#"{
  "Core": { "Id": "CORE-000507", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Define XML",
  "Check": { "all": [
    { "name": "variable_label", "operator": "not_equal_to", "value": "define_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable is incorrect",
    "Output Variables": ["define_variable_name", "define_variable_label", "variable_label"]
  }
}"#,
    )
    .expect("write define label rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 3);
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000398"
        && result.dataset == "AE"
        && result.errors[0].variables
            == vec!["variable_name", "variable_label", "library_variable_label"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000903"
        && result.dataset == "DM"
        && result.errors[0].variables == vec!["variable_name"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000507"
        && result.dataset == "VS"
        && result.error_count == 3));
}

#[test]
fn run_validation_executes_core_000929_domain_codelist_metadata_rule() {
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
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["FA"] }
    },
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["ZB"] }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
            rules_dir.join("CORE-000929.json"),
            r#"{
  "Core": { "Id": "CORE-000929", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$domain_is_custom", "operator": "domain_is_custom" },
    { "id": "$domain_lib_ccode", "operator": "codelist_terms", "codelists": ["DOMAIN"], "returntype": "code" }
  ],
  "Check": { "all": [
    { "name": "$domain_is_custom", "operator": "equal_to", "value": false },
    { "name": "define_variable_ccode", "operator": "equal_to", "value": "C66734" },
    { "name": "define_variable_codelist_coded_codes", "operator": "is_not_contained_by", "value": "$domain_lib_ccode" }
  ] },
  "Outcome": {
    "Message": "DOMAIN Code is not a published DOMAIN Code in CDISC Controlled Terminology.",
    "Output Variables": ["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
  }
}"#,
        )
        .expect("write domain codelist metadata rule");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.FA" Name="FA" Domain="FA">
    <ItemRef ItemOID="IT.FA.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.ZB" Name="ZB" Domain="ZB">
    <ItemRef ItemOID="IT.ZB.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemDef OID="IT.FA.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_FA"/>
  </ItemDef>
  <ItemDef OID="IT.ZB.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_ZB"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN_FA">
    <CodeListItem CodedValue="FA"><Alias Context="nci:ExtCodeID" Name="C00002"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
  <CodeList OID="CL.DOMAIN_ZB">
    <CodeListItem CodedValue="ZB"><Alias Context="nci:ExtCodeID" Name="C49592"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(data_dir.join(".env"), "VERSION=3-3\n").expect("write env");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000929");
    assert_eq!(failed[0].dataset, "FA");
    assert_eq!(failed[0].error_count, 1);
    assert_eq!(
        failed[0].errors[0].variables,
        vec!["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
    );
}

#[test]
fn run_validation_executes_core_000494_define_role_metadata_rule() {
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
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 }
      ],
      "records": { "DOMAIN": ["VS"], "VSTESTCD": ["SYSBP"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.VS" Name="VS" Domain="VS">
    <ItemRef ItemOID="IT.VS.DOMAIN" OrderNumber="2" Role="WRONG: Domain Identifier"/>
    <ItemRef ItemOID="IT.VS.VSTESTCD" OrderNumber="5" Role="Topic"/>
  </ItemGroupDef>
  <ItemDef OID="IT.VS.DOMAIN" Name="DOMAIN">
    <Description><TranslatedText>Domain Abbreviation</TranslatedText></Description>
  </ItemDef>
  <ItemDef OID="IT.VS.VSTESTCD" Name="VSTESTCD">
    <Description><TranslatedText>Vital Signs Test Short Name</TranslatedText></Description>
  </ItemDef>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(
            rules_dir.join("CORE-000494.json"),
            r#"{
  "Core": { "Id": "CORE-000494", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "define_variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "define_variable_role", "operator": "not_equal_to", "value": "library_variable_role" }
  ] },
  "Outcome": {
    "Message": "The Role of the variable in the define.xml does not correspond to the Role given by the Implementation Guide",
    "Output Variables": [
      "define_variable_label",
      "define_variable_name",
      "define_variable_role",
      "library_variable_name",
      "library_variable_role"
    ]
  }
}"#,
        )
        .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000494");
    assert_eq!(failed[0].dataset, "VS");
    assert_eq!(failed[0].errors[0].row, Some(1));
    assert_eq!(
        failed[0].errors[0].variables,
        vec![
            "define_variable_label",
            "define_variable_name",
            "define_variable_role",
            "library_variable_name",
            "library_variable_role"
        ]
    );
}

#[test]
fn run_validation_executes_core_000595_missing_casno_oracle_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000595.json"),
            r#"{
  "Core": { "Id": "CORE-000595", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IN"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "any": [
    { "all": [
      { "name": "UNII", "operator": "empty" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "not_exists" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "CASNO", "operator": "not_exists" },
      { "name": "UNII", "operator": "empty" }
    ] }
  ] },
  "Outcome": {
    "Message": "At least one of the UNII and CASNO variables should be present and populated for each ingredient if available.",
    "Output Variables": ["UNII", "CASNO"]
  }
}"#,
        )
        .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "in.xpt",
      "domain": "IN",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 12 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "UNII", "label": "Unique Ingredient Identifier", "type": "Char", "length": 50 }
      ],
      "records": {
        "STUDYID": ["TOB07"],
        "DOMAIN": ["IN"],
        "UNII": ["UNI2"]
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
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert!(outcome.results[0].errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_send_variable_metadata_model_column_order_rule() {
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
      "filename": "vs.xpt",
      "domain": "VS",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 },
        { "name": "VSTEST", "label": "Vital Signs Test Name", "type": "Char", "length": 40 },
        { "name": "VSNONSEN", "label": "Vital Signs Nonsense", "type": "Char", "length": 40 },
        { "name": "VSORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "VSNOTDY", "label": "Non Study Day of Vital Signs", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["VS"],
        "USUBJID": ["01"],
        "VSSEQ": [1],
        "VSTESTCD": ["WEIGHT"],
        "VSTEST": ["Weight"],
        "VSNONSEN": ["bad"],
        "VSORRES": ["80"],
        "VSNOTDY": [1]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000902.json"),
        r#"{
  "Core": { "Id": "CORE-000902", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "The variable is not an allowed variable for the underlying Observation Class",
    "Output Variables": ["variable_name"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let rows = outcome.results[0]
        .errors
        .iter()
        .map(|error| error.row)
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![Some(7), Some(9)]);
}

#[test]
fn run_validation_executes_custom_domain_variable_prefix_metadata_rule() {
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
      "filename": "zb.xpt",
      "domain": "ZB",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "ZBSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "LBORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "ZBORRESU", "label": "Original Units", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["ZB"],
        "USUBJID": ["01"],
        "ZBSEQ": [1],
        "LBORRES": ["80"],
        "ZBORRESU": ["kg"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000376.json"),
            r#"{
  "Core": { "Id": "CORE-000376", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$domain_list", "name": "DOMAIN", "operator": "distinct" },
    { "id": "$domain_is_custom", "operator": "domain_is_custom" }
  ],
  "Check": {
    "all": [
      { "name": "$domain_is_custom", "operator": "equal_to", "value": true },
      { "name": "variable_name", "operator": "is_not_contained_by", "value": ["STUDYID", "DOMAIN", "USUBJID"] },
      { "name": "variable_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$domain_list" }
    ]
  },
  "Outcome": {
    "Message": "First 2 characters of prefixed variable within custom domain do not match the DOMAIN value.",
    "Output Variables": ["$domain_list", "variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "ZB");
    assert_eq!(outcome.results[0].errors[0].row, Some(5));
}
