use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_core_000750_detects_unique_set_duplicates_across_split_domains() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000750.json"),
        r#"{
  "Core": { "Id": "CORE-000750", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"], "Exclude": ["TS"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "non_empty" },
      { "name": "--SEQ", "operator": "exists" },
      { "name": "--SEQ", "operator": "is_not_unique_set", "value": ["DOMAIN", "USUBJID"] }
    ]
  },
  "Outcome": {
    "Message": "--SEQ is not unique per subject and domain.",
    "Output Variables": ["USUBJID", "--SEQ", "POOLID"]
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
      "filename": "lbae.xpt",
      "domain": "LBAE",
      "records": {
        "STUDYID": ["S"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [4],
        "LBTESTCD": ["SURVSTAT"]
      }
    },
    {
      "filename": "lbds.xpt",
      "domain": "LBDS",
      "records": {
        "STUDYID": ["S"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [4],
        "LBTESTCD": ["SURVSTAT"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    let issues = outcome
        .results
        .iter()
        .flat_map(|result| result.errors.iter())
        .map(|issue| {
            (
                issue.dataset.as_str(),
                issue.row,
                issue.usubjid.as_deref(),
                issue.seq.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        issues,
        vec![
            ("LBAE", Some(1), Some("SUBJ1"), Some("4")),
            ("LBDS", Some(1), Some("SUBJ1"), Some("4")),
        ]
    );
}

#[test]
fn run_validation_core_000495_omits_unique_set_group_locator_from_variables() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000495.json"),
        r#"{
  "Core": { "Id": "CORE-000495", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MA"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "MAORRES", "operator": "non_empty" },
      { "name": "MASPEC", "operator": "non_empty" },
      {
        "name": "MAORRES",
        "operator": "is_not_unique_set",
        "value": ["USUBJID", "MATESTCD", "MASPEC"]
      }
    ]
  },
  "Outcome": {
    "Message": "The Macroscopic Finding test result is not unique for this subject and test.",
    "Output Variables": ["USUBJID", "MATESTCD", "MAORRES", "MASPEC"]
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
      "filename": "ma.xpt",
      "domain": "MA",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["MA", "MA"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "MASEQ": [1, 2],
        "MATESTCD": ["GROSPATH", "GROSPATH"],
        "MAORRES": ["Discoloration dark, mucosal", "Discoloration dark, mucosal"],
        "MASPEC": ["SKIN", "SKIN"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    for issue in &result.errors {
        assert_eq!(issue.variables, vec!["MATESTCD", "MAORRES", "MASPEC"]);
        assert_eq!(issue.usubjid.as_deref(), Some("SUBJ1"));
    }
}

#[test]
fn run_validation_core_000526_includes_subject_locator_in_unique_set_variables() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000526.json"),
        r#"{
  "Core": { "Id": "CORE-000526", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["PP"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "name": "PPTESTCD",
        "operator": "is_not_unique_set",
        "value": ["USUBJID", "PPCAT", "PPSPEC", "PPTPTREF"]
      },
      {
        "name": "PPTESTCD",
        "operator": "is_not_unique_set",
        "value": ["POOLID", "PPCAT", "PPSPEC", "PPTPTREF"]
      }
    ]
  },
  "Outcome": {
    "Message": "The combination of POOLID or USUBJID, PPTESTCD, PPCAT, PPSPEC, and PPTPTREF is not unique",
    "Output Variables": ["POOLID", "PPTESTCD", "PPCAT", "PPSPEC", "PPTPTREF"]
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
      "filename": "pp.xpt",
      "domain": "PP",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["PP", "PP"],
        "USUBJID": ["123101", "123101"],
        "POOLID": ["", ""],
        "PPSEQ": [1, 2],
        "PPTESTCD": ["AUCINT", "AUCINT"],
        "PPCAT": ["XYZ-123", "XYZ-123"],
        "PPSPEC": ["PLASMA", "PLASMA"],
        "PPTPTREF": ["Day 1 dose", "Day 1 dose"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    for issue in &result.errors {
        assert!(issue.variables.contains(&"USUBJID".to_owned()));
        assert_eq!(issue.usubjid.as_deref(), Some("123101"));
    }
}
