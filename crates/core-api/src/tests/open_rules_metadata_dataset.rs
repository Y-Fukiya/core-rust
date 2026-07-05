use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};
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
