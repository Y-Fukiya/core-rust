use std::fs;

use core_api::{run_validation, ValidateRequest};
use core_engine::ExecutionStatus;
use serde_json::{json, Value};
use tempfile::tempdir;

#[test]
fn core_000037_reports_source_records_for_invalid_svpresp() {
    assert_sv_record_locators(
        "CORE-000037",
        json!([
            {"name": "SVPRESP", "operator": "not_equal_to", "value": "Y", "value_is_literal": true},
            {"name": "SVPRESP", "operator": "non_empty"}
        ]),
        &["SVPRESP"],
        &[(29, "N", ""), (59, "UNK", "")],
    );
}

#[test]
fn core_000038_reports_source_records_for_unplanned_occurred_visits() {
    assert_sv_record_locators(
        "CORE-000038",
        json!([
            {"name": "SVOCCUR", "operator": "non_empty"},
            {"name": "SVPRESP", "operator": "not_equal_to", "value": "Y", "value_is_literal": true}
        ]),
        &["SVPRESP", "SVOCCUR"],
        &[(1, "Yes", "Y"), (29, "N", "Y"), (60, "No", "Y")],
    );
}

fn assert_sv_record_locators(
    rule_id: &str,
    checks: Value,
    variables: &[&str],
    invalid_visits: &[(usize, &str, &str)],
) {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("rule.json");
    let rule = json!({
        "Core": {"Id": rule_id, "Status": "Published"},
        "Scope": {"Domains": {"Include": ["SV"]}},
        "Sensitivity": "Record",
        "Rule Type": "Record Data",
        "Check": {"all": checks},
        "Outcome": {"Message": "Invalid visit.", "Output Variables": variables}
    });
    fs::write(&rule_path, rule.to_string()).expect("write rule");

    let dataset_path = dir.path().join("sv.csv");
    let mut csv = csv::Writer::from_path(&dataset_path).expect("create SV dataset");
    csv.write_record(["DOMAIN", "USUBJID", "SVPRESP", "SVOCCUR"])
        .expect("write header");
    for row in 1..=60 {
        let (_, presp, occur) = invalid_visits
            .iter()
            .find(|&&(record, _, _)| record == row)
            .copied()
            .unwrap_or(if row % 2 == 0 {
                (row, "Y", "Y")
            } else {
                (row, "", "")
            });
        let subject = format!("SUBJ{row:02}");
        csv.write_record(["SV", &subject, presp, occur])
            .expect("write visit");
    }
    csv.flush().expect("flush dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rule_path],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let [result] = outcome.results.as_slice() else {
        panic!("expected one validation result, got {:?}", outcome.results);
    };
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, invalid_visits.len());
    let actual_rows: Vec<_> = result.errors.iter().map(|issue| issue.row).collect();
    let expected_rows: Vec<_> = invalid_visits
        .iter()
        .map(|&(row, _, _)| Some(row))
        .collect();
    assert_eq!(
        actual_rows, expected_rows,
        "CSV record locators for {rule_id}"
    );
    for (issue, &(row, _, _)) in result.errors.iter().zip(invalid_visits) {
        assert_eq!(issue.rule_id, rule_id);
        assert_eq!(issue.dataset, "SV");
        assert_eq!(issue.domain.as_deref(), Some("SV"));
        assert_eq!(issue.variables, variables);
        assert_eq!(issue.usubjid, Some(format!("SUBJ{row:02}")));
        assert_eq!(issue.seq, None);
    }
}

#[test]
fn core_000025_reports_csv_record_numbers_separately_from_sequence() {
    assert_csv_locators(
        "CORE-000025",
        "IE",
        "Record",
        json!([{"name": "IESTRESC", "operator": "not_equal_to", "value": "IEORRES"}]),
        &["IEORRES", "IESTRESC", "IETESTCD"],
        "DOMAIN,USUBJID,IESEQ,IEORRES,IESTRESC,IETESTCD\n\
         IE,S1,73,Y,Yup,INCL03\n\
         IE,S2,11,N,N,INCL04\n\
         IE,S3,4,Nope,Nah,INCL05\n",
        &[
            (Some(1), Some("S1"), Some("73")),
            (Some(3), Some("S3"), Some("4")),
        ],
    );
}

#[test]
fn core_000223_reports_the_failing_record_not_the_previous_record() {
    assert_csv_locators(
        "CORE-000223",
        "DM",
        "Record",
        json!([
            {"name": "ACTARMCD", "operator": "empty"},
            {"name": "ARMNRS", "operator": "empty"}
        ]),
        &["ACTARMCD", "ARMNRS"],
        "DOMAIN,USUBJID,ACTARMCD,ARMNRS\n\
         DM,S1,ARM A,\n\
         DM,S2,,\n\
         DM,S3,,SCREEN FAILURE\n\
         DM,S4,,\n",
        &[(Some(2), Some("S2"), None), (Some(4), Some("S4"), None)],
    );
}

#[test]
fn core_000186_keeps_duplicate_record_locators_out_of_dataset_findings() {
    type Locator = (Option<usize>, Option<&'static str>, Option<&'static str>);
    let cases: [(&str, &[Locator]); 2] = [
        (
            "Record",
            &[(Some(1), Some("A-01"), None), (Some(4), Some("A-03"), None)],
        ),
        ("Dataset", &[(None, None, None)]),
    ];

    for (sensitivity, expected) in cases {
        assert_csv_locators(
            "CORE-000186",
            "DM",
            sensitivity,
            json!([{"name": "SUBJID", "operator": "is_not_unique_set", "value": ["STUDYID"]}]),
            &["SUBJID"],
            "DOMAIN,STUDYID,USUBJID,SUBJID\n\
             DM,A,A-01,01\n\
             DM,B,B-01,01\n\
             DM,A,A-02,02\n\
             DM,A,A-03,01\n",
            expected,
        );
    }
}

fn assert_csv_locators(
    rule_id: &str,
    domain: &str,
    sensitivity: &str,
    checks: Value,
    variables: &[&str],
    dataset: &str,
    expected: &[(Option<usize>, Option<&str>, Option<&str>)],
) {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("rule.json");
    let rule = json!({
        "Core": {"Id": rule_id, "Status": "Published"},
        "Scope": {"Domains": {"Include": [domain]}},
        "Sensitivity": sensitivity,
        "Rule Type": "Record Data",
        "Check": {"all": checks},
        "Outcome": {"Message": "Invalid record data.", "Output Variables": variables}
    });
    fs::write(&rule_path, rule.to_string()).expect("write rule");

    let dataset_path = dir
        .path()
        .join(format!("{}.csv", domain.to_ascii_lowercase()));
    fs::write(&dataset_path, dataset).expect("write dataset");
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rule_path],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let [result] = outcome.results.as_slice() else {
        panic!("expected one validation result, got {:?}", outcome.results);
    };
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, expected.len());
    let actual: Vec<_> = result
        .errors
        .iter()
        .map(|issue| {
            assert_eq!(issue.rule_id, rule_id);
            assert_eq!(issue.dataset, domain);
            assert_eq!(issue.domain.as_deref(), Some(domain));
            assert_eq!(issue.variables, variables);
            (issue.row, issue.usubjid.as_deref(), issue.seq.as_deref())
        })
        .collect();
    assert_eq!(actual, expected, "{rule_id} ({sensitivity})");
}
