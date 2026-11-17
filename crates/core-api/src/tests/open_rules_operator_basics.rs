use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_checks_earliest_repeated_consent_against_dm() {
    let dir = tempdir().expect("tempdir");
    let rule = dir.path().join("rule.json");
    let data = dir.path().join("datasets.json");
    fs::write(
        &rule,
        r#"{
  "Core": {"Id": "CORE-CONSENT-TEST", "Status": "Published"},
  "Scope": {"Domains": {"Include": ["DS"]}},
  "Rule Type": "Record Data", "Sensitivity": "Record",
  "Match Datasets": [{"Name": "DM", "Keys": ["USUBJID"]}],
  "Operations": [{"operator": "min_date", "domain": "DS", "name": "DSSTDTC",
                  "group": ["USUBJID"], "id": "$min_ds_dsstdtc"}],
  "Check": {"all": [
    {"name": "DSDECOD", "operator": "present_on_multiple_rows_within", "within": "USUBJID"},
    {"name": "DSDECOD", "operator": "equal_to", "value": "INFORMED CONSENT OBTAINED", "value_is_literal": true},
    {"name": "$min_ds_dsstdtc", "operator": "date_not_equal_to", "value": "RFICDTC"},
    {"name": "$min_ds_dsstdtc", "operator": "date_equal_to", "value": "DSSTDTC"}
  ]},
  "Outcome": {"Output Variables": ["DSDECOD", "RFICDTC", "DSSTDTC", "$min_ds_dsstdtc"], "Message": "Consent date mismatch"}
}"#,
    )
    .expect("write rule");

    for (consent_date, expected_rows) in [("2024-01-03", vec![Some(2)]), ("2024-01-01", vec![])] {
        let consent = "INFORMED CONSENT OBTAINED";
        let package = serde_json::json!({"datasets": [
            {"filename": "ds.xpt", "domain": "DS", "records": {
                "USUBJID": ["A", "A", "B", "B"],
                "DSDECOD": [consent, consent, consent, "COMPLETED"],
                "DSSTDTC": ["2024-01-02", "2024-01-01", "2024-01-01", "2024-01-02"]
            }},
            {"filename": "dm.xpt", "domain": "DM", "records": {
                "USUBJID": ["A", "B"], "RFICDTC": [consent_date, "2024-01-03"]
            }}
        ]});
        fs::write(&data, package.to_string()).expect("write datasets");
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rule.clone()],
            dataset_paths: vec![data.clone()],
            ..Default::default()
        })
        .expect("validate");
        let [result] = outcome.results.as_slice() else {
            panic!("expected one rule result: {:?}", outcome.results);
        };
        assert_eq!(
            result.execution_status,
            if expected_rows.is_empty() {
                ExecutionStatus::Passed
            } else {
                ExecutionStatus::Failed
            },
            "{result:?}"
        );
        assert_eq!(
            result
                .errors
                .iter()
                .map(|issue| issue.row)
                .collect::<Vec<_>>(),
            expected_rows
        );
    }
}

#[test]
fn run_validation_checks_idvar_against_referenced_physical_columns() {
    let dir = tempdir().expect("tempdir");
    let rule = dir.path().join("rule.json");
    let data = dir.path().join("datasets.json");
    fs::write(
        &rule,
        r#"{
  "Core": {"Id": "CORE-IDVAR-TEST", "Status": "Published"},
  "Scope": {"Domains": {"Include": ["SUPP--"]}},
  "Rule Type": "Record Data", "Sensitivity": "Record",
  "Match Datasets": [{"Name": "SUPP--", "Child": true, "Keys": ["USUBJID", "IDVAR", "IDVARVAL"]}],
  "Operations": [{"operator": "referenced_domain_variable_names", "domain": "SUPP--",
                  "name": "RDOMAIN", "id": "$rdomain_variables"}],
  "Check": {"all": [
    {"name": "RDOMAIN", "operator": "exists"},
    {"name": "IDVAR", "operator": "non_empty"},
    {"name": "IDVAR", "operator": "is_not_contained_by", "value": "$rdomain_variables"}
  ]},
  "Outcome": {"Output Variables": ["IDVAR", "RDOMAIN"], "Message": "No such variable"}
}"#,
    )
    .expect("write rule");

    let cases = [
        (Some("LBSEK"), true, ExecutionStatus::Failed),
        (Some("LBSEQ"), true, ExecutionStatus::Passed),
        (None, true, ExecutionStatus::Skipped),
        (Some("LBSEQ"), false, ExecutionStatus::Skipped),
    ];
    for (idvar, include_rdomain, expected) in cases {
        let mut supp = serde_json::json!({"USUBJID": ["A", "A", "A"], "IDVARVAL": ["1", "2", ""]});
        if let Some(name) = idvar {
            supp["IDVAR"] = serde_json::json!([name, "LBTEST", ""]);
        }
        if include_rdomain {
            supp["RDOMAIN"] = serde_json::json!(["LB", "LB", "LB"]);
        }
        let package = serde_json::json!({"datasets": [
            {"filename": "lb1.xpt", "domain": "LB", "records": {"LBSEQ": [1]}},
            {"filename": "lb2.xpt", "domain": "LB", "records": {"LBTEST": ["Test"]}},
            {"filename": "suppdm.xpt", "domain": "SUPPDM", "records": {
                "USUBJID": ["A"], "RDOMAIN": ["DM"], "IDVAR": [""], "IDVARVAL": [""]
            }},
            {"filename": "supplb.xpt", "domain": "SUPPLB", "records": supp}
        ]});
        fs::write(&data, package.to_string()).expect("write datasets");
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rule.clone()],
            dataset_paths: vec![data.clone()],
            ..Default::default()
        })
        .expect("validate");
        assert!(!outcome.results.is_empty());
        if expected == ExecutionStatus::Failed {
            let issues = outcome
                .results
                .iter()
                .flat_map(|result| &result.errors)
                .collect::<Vec<_>>();
            let [issue] = issues.as_slice() else {
                panic!("expected only the invalid IDVAR: {:?}", outcome.results);
            };
            assert_eq!(issue.dataset.to_ascii_uppercase(), "SUPPLB");
            assert_eq!(issue.row, Some(1));
            assert_eq!(issue.variables, ["IDVAR", "RDOMAIN"]);
        } else {
            assert!(
                outcome
                    .results
                    .iter()
                    .all(|result| result.execution_status == expected),
                "{:?}",
                outcome.results
            );
        }
    }
}

#[test]
fn run_validation_executes_target_is_not_sorted_by_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SORT-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-SORT-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "AESTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "AESEQ is not chronological" }
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
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "AESEQ": [1, 3, 2],
        "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_executes_empty_within_except_last_row_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-END-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-END-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SEENDTC",
    "operator": "empty_within_except_last_row",
    "ordering": "SESTDTC",
    "value": "USUBJID"
  },
  "Outcome": { "Message": "SEENDTC is empty before the last row" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SESEQ": [1, 2, 3],
        "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"],
        "SEENDTC": ["2024-01-02", "", ""]
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_not_present_on_multiple_rows_within_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let rule_file = rules_dir.join("CORE-REL-OPERATOR.json");
    let mut rule: serde_json::Value = serde_json::from_str(
        r#"{
  "Core": { "Id": "CORE-REL-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "not_present_on_multiple_rows_within",
    "within": "USUBJID"
  },
  "Outcome": { "Message": "RELID must appear on multiple rows" }
}"#,
    )
    .expect("parse rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    for variables in [&["USUBJID", "RELID"][..], &["RELID"][..]] {
        if variables.len() == 1 {
            rule["Outcome"]["Output Variables"] = serde_json::json!(["RELID"]);
        }
        fs::write(
            &rule_file,
            serde_json::to_vec(&rule).expect("serialize rule"),
        )
        .expect("write rule");
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path.clone()],
            output_dir: None,
            open_rules_oracle_compat: true,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.skipped_reason, None);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].row, Some(3));
        assert_eq!(result.errors[0].usubjid.as_deref(), Some("SUBJ1"));
        assert_eq!(result.errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_is_not_unique_set_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-UNIQUE-SET.json"),
        r#"{
  "Core": { "Id": "CORE-UNIQUE-SET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be unique within subject" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
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

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}
