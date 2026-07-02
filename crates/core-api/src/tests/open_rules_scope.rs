use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;

#[test]
fn run_validation_filters_execution_datasets_by_domain_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-DOMAIN-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["MS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "MS",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must not be MS" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "ms.xpt",
      "domain": "MS",
      "records": { "USUBJID": ["S1"], "MSSEQ": [1], "DOMAIN": ["MS"] }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "AE");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
}

#[test]
fn run_validation_domain_scope_matches_supp_placeholder_domains() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SUPP-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-SUPP-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "QNAM",
    "operator": "matches_regex",
    "value": "^[0-9]"
  },
  "Outcome": { "Message": "QNAM starts with a number" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "supplb.xpt",
      "domain": "SUPPLB",
      "records": {
        "USUBJID": ["S1"],
        "IDVAR": ["LBSEQ"],
        "IDVARVAL": ["1"],
        "QNAM": ["5BIOSIG"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "SUPPLB");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_filters_execution_datasets_by_class_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-CLASS-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-CLASS-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "LB",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must be LB" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": { "USUBJID": ["S1"], "LBSEQ": [1], "DOMAIN": ["LB"] }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": { "USUBJID": ["S1"], "FASEQ": [1], "DOMAIN": ["FA"] }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert_eq!(outcome.results[0].dataset, "LB");
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[1].dataset, "FA");
    assert_eq!(
        outcome.results[1].execution_status,
        ExecutionStatus::Passed,
        "{:?}",
        outcome.results[1]
    );
}

#[test]
fn run_validation_scans_forbidden_send_domain_placeholder_variables_across_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000794.json"),
        r#"{
  "Core": { "Id": "CORE-000794", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "--PTCD", "operator": "exists" },
  "Outcome": { "Message": "--PTCD must not be present" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "variables": [
        { "name": "STUDYID" },
        { "name": "DMPTCD" }
      ],
      "records": { "STUDYID": ["S1"], "DMPTCD": ["1"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID" },
        { "name": "VSPTCD" }
      ],
      "records": { "STUDYID": ["S1"], "VSPTCD": ["1"] }
    },
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "variables": [
        { "name": "STUDYID" },
        { "name": "TXSEQ" }
      ],
      "records": { "STUDYID": ["S1"], "TXSEQ": [1] }
    }
  ]
}"#,
    )
    .expect("write dataset");

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
    assert_eq!(failed[0].dataset, "DM");
    assert_eq!(failed[0].errors[0].row, None);
    assert_eq!(failed[0].errors[0].variables, vec!["DMPTCD"]);
    assert_eq!(failed[1].dataset, "VS");
    assert_eq!(failed[1].errors[0].row, None);
    assert_eq!(failed[1].errors[0].variables, vec!["VSPTCD"]);
}
