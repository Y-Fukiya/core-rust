use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::helpers::write_dataset;
use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_filters_rules_by_standard_and_version() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-STANDARD-34.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write matching standard rule");
    fs::write(
        rules_dir.join("CORE-STANDARD-33.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "CM"
  },
  "Outcome": { "Message": "DOMAIN must be CM" }
}"#,
    )
    .expect("write nonmatching standard rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-34");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_reports_explicit_rule_standard_mismatch_as_skipped() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-STANDARD-33.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write nonmatching standard rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-STANDARD-33".to_owned()],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-33");
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::StandardMismatch)
    );
}

#[test]
fn run_validation_classifies_known_standard_filter_fixture_gaps_as_oracle_gaps() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-000478.json"),
        r#"{
  "Core": { "Id": "CORE-000478", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "not_equal_to", "value": "AE" },
  "Outcome": { "Message": "known standard filter fixture gap" }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000478".to_owned()],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.0".to_owned()),
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
fn run_validation_treats_usdm_30_rules_as_compatible_with_usdm_40_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-USDM-30.json"),
        r#"{
  "Core": { "Id": "CORE-USDM-30", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "USDM", "Version": "3.0" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write USDM 3.0 rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("USDM".to_owned()),
        standard_version: Some("4.0".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-USDM-30");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_sdtmig_34_rules_as_compatible_with_sdtmig_33_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-SDTMIG-34.json"),
        r#"{
  "Core": { "Id": "CORE-SDTMIG-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write SDTMIG 3.4 rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.3".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-SDTMIG-34");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_send_family_versions_as_compatible_for_open_rules_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-SEND-FAMILY.json"),
        r#"{
  "Core": { "Id": "CORE-SEND-FAMILY", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG-DART", "Version": "1.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write SEND family rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-SEND-FAMILY");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_core_000119_tig_sdtm_as_compatible_with_sendig_31() {
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
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "ARMCD", "label": "Planned Arm Code", "type": "Char", "length": 20 },
        { "name": "ARM", "label": "Description of Planned Arm", "type": "Char", "length": 40 }
      ],
      "records": { "ARMCD": [""], "ARM": ["PLACEBO"] }
    }
  ]
}"#,
    )
    .expect("write data");

    fs::write(
        rules_dir.join("CORE-000119.json"),
        r#"{
  "Core": { "Id": "CORE-000119", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "ARMCD", "operator": "empty" },
    { "name": "ARM", "operator": "non_empty" }
  ] },
  "Outcome": {
    "Message": "ARM is populated, when ARMCD is NULL",
    "Output Variables": ["ARMCD", "ARM"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000119");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_tig_rules_as_compatible_with_sdtmig_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-TIG.json"),
        r#"{
  "Core": { "Id": "CORE-TIG", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write TIG rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-TIG");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}
