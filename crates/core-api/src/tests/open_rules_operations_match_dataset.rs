use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use serde_json::json;
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
fn run_validation_reports_aesosp_only_on_the_referenced_ae_record() {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("CORE-000597.json");
    let dataset_path = dir.path().join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1", "S1", "S1", "S2", "S2", "S3"],
        "AESEQ": [7, 8, 9, 10, 7, 8, 9],
        "AEREFID": ["A", "B", "C", "D", "E", "F", "0009"],
        "AESMIE": [null, "N", "Y", "N", "N", "N", "N"],
        "AETERM": ["A", "B", "C", "D", "E", "F", "G"]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "RDOMAIN": ["AE", "AE", "AE", "AE", "AE", "AE", "AE", "AE"],
        "USUBJID": ["S2", "S1", "S1", "S1", "S1", "S3", "S4", "S3"],
        "IDVAR": ["AESEQ", "AESEQ", "AESEQ", "AESEQ", "AESEQ", "AESEQ", "AESEQ", "AEREFID"],
        "IDVARVAL": ["0008", "10", "0007", "8", "9", "404", "7", "9"],
        "QNAM": ["AESOSP", "OTHER", "AESOSP", "AESOSP", "AESOSP", "AESOSP", "AESOSP", "AESOSP"],
        "QVAL": ["second subject", "other qualifier", "missing flag", "", "flagged", "orphan", "no subject", "different character key"]
      }
    }
  ]
}"#,
    )
    .expect("write AE and SUPPAE data");

    let versions = [
        (
            json!({ "all": [
                {"any": [
                    {"name": "AESMIE", "operator": "not_equal_to", "value": "Y", "value_is_literal": true},
                    {"name": "AESMIE", "operator": "not_exists"}
                ]},
                {"name": "AESOSP", "operator": "non_empty"}
            ] }),
            vec!["AESOSP", "AETERM", "AESMIE"],
            vec![
                (Some(1), Some("S1"), Some("7")),
                (Some(6), Some("S2"), Some("8")),
            ],
        ),
        (
            json!({ "all": [
                {"name": "QNAM", "operator": "equal_to", "value": "AESOSP", "value_is_literal": true},
                {"name": "AESMIE", "operator": "not_equal_to", "value": "Y"}
            ] }),
            vec!["QNAM", "AESMIE"],
            vec![
                (Some(1), Some("S1"), Some("7")),
                (Some(2), Some("S1"), Some("8")),
                (Some(6), Some("S2"), Some("8")),
            ],
        ),
    ];

    for (check, variables, expected) in versions {
        let rule = json!({
            "Core": {"Id": "CORE-000597", "Status": "Published"},
            "Scope": {"Domains": {"Include": ["AE"]}},
            "Rule Type": "Record Data",
            "Sensitivity": "Record",
            "Match Datasets": [{"Name": "SUPPAE", "Keys": ["USUBJID"]}],
            "Check": check,
            "Outcome": {"Message": "AESOSP requires AESMIE=Y", "Output Variables": &variables}
        });
        fs::write(&rule_path, rule.to_string()).expect("write rule");
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rule_path.clone()],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");
        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, expected.len(), "{result:?}");
        let mut identities = Vec::new();
        for issue in &result.errors {
            assert_eq!(issue.rule_id, "CORE-000597");
            assert_eq!(issue.domain.as_deref(), Some("AE"));
            assert_eq!(issue.variables, variables);
            identities.push((issue.row, issue.usubjid.as_deref(), issue.seq.as_deref()));
        }
        identities.sort_unstable();
        assert_eq!(identities, expected);
    }
}

#[test]
fn run_validation_reports_sv_element_order_on_original_sv_records() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    let rule = r#"{
  "Core": { "Id": "CORE-000784", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SE", "Keys": ["USUBJID"] }
  ],
  "Check": { "all": [
    { "name": "TAETORD", "operator": "exists" },
    { "name": "SESTDTC", "operator": "date_less_than_or_equal_to", "value": "SVSTDTC" },
    { "name": "SVSTDTC", "operator": "date_less_than_or_equal_to", "value": "SEENDTC" },
    { "name": "TAETORD", "operator": "not_equal_to", "value": "SE.TAETORD" }
  ] },
  "Outcome": {
    "Message": "SV TAETORD does not match the active SE element",
    "Output Variables": ["SESTDTC", "SEENDTC", "SVSTDTC", "TAETORD", "SE.TAETORD"]
  }
}"#;

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2", "SUBJ1", "SUBJ3"],
        "SVSEQ": [null, null, null, null, 72, 73],
        "SVSTDTC": ["2020-01-03", "2020-01-06T6:10", "2020-01-06T06:10", "2020-01-06", "2020-01-06T06:10", "2020-01-06"],
        "TAETORD": [1, 9, 9, 9, 9, 9]
      }
    },
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ1", "SUBJ3"],
        "SESEQ": [11, 12, 13, 14, 15],
        "SESTDTC": ["2020-01-01", "2020-01-05", "2020-01-06T6:00", "2020-01-05", "2020-01-06T23:30-02:00"],
        "SEENDTC": ["2020-01-31", "2020-01-31", "2020-01-31", "2020-01-06T6:30", "2020-01-31"],
        "TAETORD": [1, 2, 3, 4, 1]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let accepted_rule = rule.replace(
        r#""name": "SESTDTC", "operator": "date_less_than_or_equal_to", "value": "SVSTDTC""#,
        r#""name": "SVSTDTC", "operator": "date_greater_than_or_equal_to", "value": "SESTDTC""#,
    );
    for rule in [rule, &accepted_rule] {
        fs::write(rules_dir.join("CORE-000784.json"), rule).expect("write rule");
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 4, "{result:?}");
        let mut identities = Vec::new();
        for issue in &result.errors {
            assert_eq!(issue.rule_id, "CORE-000784");
            assert_eq!(issue.domain.as_deref(), Some("SV"));
            assert_eq!(
                issue.variables,
                vec!["SESTDTC", "SEENDTC", "SVSTDTC", "TAETORD", "SE.TAETORD"]
            );
            identities.push((issue.row, issue.usubjid.as_deref(), issue.seq.as_deref()));
        }
        identities.sort_unstable();
        assert_eq!(
            identities,
            vec![
                (Some(3), Some("SUBJ1"), None),
                (Some(3), Some("SUBJ1"), None),
                (Some(5), Some("SUBJ1"), Some("72")),
                (Some(5), Some("SUBJ1"), Some("72")),
            ]
        );
    }
}

#[test]
fn run_validation_preserves_svseq_when_sv_element_columns_are_prejoined() {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("CORE-000784.json");
    let dataset_path = dir.path().join("datasets.json");
    fs::write(
        &rule_path,
        r#"{
  "Core": { "Id": "CORE-000784", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "SESTDTC", "operator": "date_less_than_or_equal_to", "value": "SVSTDTC" },
    { "name": "TAETORD", "operator": "not_equal_to", "value": "SE.TAETORD" }
  ] },
  "Outcome": { "Message": "SV TAETORD does not match the active SE element" }
}"#,
    )
    .expect("write rule");
    fs::write(
        &dataset_path,
        r#"{"datasets": [{"filename": "sv.xpt", "domain": "SV", "records": {
  "USUBJID": ["SUBJ1"], "SVSEQ": [72], "SESEQ": [99],
  "SVSTDTC": ["2020-01-06"], "SESTDTC": ["2020-01-05"],
  "TAETORD": [9], "SE.TAETORD": [1]
}}]}"#,
    )
    .expect("write data");
    let result = run_validation(ValidateRequest {
        rule_paths: vec![rule_path],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    assert_eq!(result.results[0].execution_status, ExecutionStatus::Failed);
    let [issue] = result.results[0].errors.as_slice() else {
        panic!("expected one issue: {:?}", result.results);
    };
    assert_eq!(issue.row, Some(1));
    assert_eq!(issue.seq.as_deref(), Some("72"));
}
