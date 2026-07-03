use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_joins_single_match_dataset_with_prefixed_condition_column() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000249.json"),
        r#"{
  "Core": { "Id": "CORE-000249", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "TV", "Keys": ["VISITNUM"] }
  ],
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visitnum",
      "name": "VISITNUM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "VISITDY", "operator": "exists" },
      { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" },
      { "name": "VISITDY", "operator": "not_equal_to", "value": "TV.VISITDY" }
    ]
  },
  "Outcome": {
    "Message": "Visit Day cannot be found in Trial Visit (TV) domain",
    "Output Variables": ["VISITDY", "VISITNUM"]
  }
}"#,
    )
    .expect("write visitdy rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "LBSEQ": [1, 2],
        "VISITNUM": ["100", "200"],
        "VISITDY": ["-14", "-2"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["TV", "TV"],
        "VISITNUM": ["100", "200"],
        "VISITDY": ["-14", "1"]
      }
    }
  ]
}"#,
    )
    .expect("write visitdy data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1, "{result:?}");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(result.errors[0].seq.as_deref(), Some("2"));
    assert_eq!(
        result.errors[0].variables,
        vec!["VISITDY".to_owned(), "VISITNUM".to_owned()]
    );
}

#[test]
fn run_validation_keeps_unprefixed_match_columns_for_prefixed_conflicts() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000784.json"),
        r#"{
  "Core": { "Id": "CORE-000784", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SE", "Keys": ["USUBJID"] }
  ],
  "Check": { "all": [
    { "name": "TAETORD", "operator": "exists" },
    { "name": "SVSTDTC", "operator": "date_greater_than_or_equal_to", "value": "SESTDTC" },
    { "name": "SVSTDTC", "operator": "date_less_than_or_equal_to", "value": "SEENDTC" },
    { "name": "TAETORD", "operator": "not_equal_to", "value": "SE.TAETORD" }
  ] },
  "Outcome": {
    "Message": "SV TAETORD does not match the active SE element",
    "Output Variables": ["SESTDTC", "SEENDTC", "SVSTDTC", "TAETORD", "SE.TAETORD"]
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
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["SV", "SV"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "VISITNUM": [1, 2],
        "SVSTDTC": ["2020-01-05", "2020-01-06"],
        "TAETORD": [1, 9]
      }
    },
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "SESTDTC": ["2020-01-01", "2020-02-01"],
        "SEENDTC": ["2020-01-31", "2020-02-28"],
        "TAETORD": [1, 2]
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

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1, "{result:?}");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(
        result.errors[0].variables,
        vec![
            "SESTDTC".to_owned(),
            "SEENDTC".to_owned(),
            "SVSTDTC".to_owned(),
            "TAETORD".to_owned(),
            "SE.TAETORD".to_owned(),
        ]
    );
}
