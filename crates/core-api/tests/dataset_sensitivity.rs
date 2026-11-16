use std::fs;

use core_api::{run_validation, ValidateRequest};
use core_engine::ExecutionStatus;
use serde_json::{json, Value};
use tempfile::tempdir;

#[test]
fn dataset_sensitivity_reports_each_dataset_independently() {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("CORE-000013.json");
    fs::write(
        &rule_path,
        serde_json::to_vec(&json!({
            "Core": {"Id": "CORE-000013", "Status": "Published"},
            "Scope": {"Domains": {"Include": ["AE"]}},
            "Sensitivity": "Dataset",
            "Rule Type": "Record Data",
            "Check": {"all": [{"name": "AESTAT", "operator": "exists"}]},
            "Outcome": {
                "Message": "AESTAT must not be present in AE dataset.",
                "Output Variables": ["AESTAT"]
            }
        }))
        .expect("serialize rule"),
    )
    .expect("write rule");
    let datasets = [
        ("ae1.csv", Some(json!(["NOT DONE", "NOT DONE", "NOT DONE"]))),
        ("ae2.csv", Some(json!(["NOT DONE", "", null]))),
        ("ae3.csv", Some(json!(["", null, " "]))),
        ("ae4.csv", None),
    ]
    .into_iter()
    .map(|(filename, aestat)| {
        let mut records = json!({
            "DOMAIN": ["AE", "AE", "AE"],
            "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
            "AESEQ": [1, 2, 3]
        });
        if let Some(aestat) = aestat {
            records["AESTAT"] = aestat;
        }
        json!({"filename": filename, "domain": "AE", "records": records})
    })
    .collect::<Vec<_>>();
    let dataset_path = dir.path().join("datasets.json");
    fs::write(
        &dataset_path,
        serde_json::to_vec(&json!({"datasets": datasets})).expect("serialize datasets"),
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rule_path],
        dataset_paths: vec![dataset_path],
        output_dir: Some(dir.path().join("reports")),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 4);
    for (result, expected_count) in outcome.results.iter().zip([1, 1, 1, 0]) {
        assert_eq!(result.dataset, "AE");
        assert_eq!(result.error_count, expected_count);
        assert_eq!(result.errors.len(), expected_count);
        assert_eq!(
            result.execution_status,
            if expected_count == 0 {
                ExecutionStatus::Passed
            } else {
                ExecutionStatus::Failed
            }
        );
        for issue in &result.errors {
            assert_eq!(issue.rule_id, "CORE-000013");
            assert_eq!(issue.dataset, "AE");
            assert_eq!(issue.domain.as_deref(), Some("AE"));
            assert_eq!(issue.variables, vec!["AESTAT"]);
            assert_eq!(issue.row, None);
            assert_eq!(issue.usubjid, None);
            assert_eq!(issue.seq, None);
        }
    }

    let reports = outcome.reports.expect("reports");
    let report: Value = serde_json::from_slice(
        &fs::read(reports.json.expect("json report")).expect("read json report"),
    )
    .expect("parse json report");
    assert_eq!(report["summary"]["error_count"], 3);
    assert_eq!(report["summary"]["failed"], 3);
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(
        report["results"],
        serde_json::to_value(&outcome.results).unwrap()
    );

    let mut csv =
        csv::Reader::from_path(reports.csv.expect("csv report")).expect("read csv report");
    let rows = csv
        .deserialize::<std::collections::BTreeMap<String, String>>()
        .collect::<Result<Vec<_>, _>>()
        .expect("parse csv report");
    assert_eq!(rows.len(), 4);
    for (row, expected_count) in rows.iter().zip(["1", "1", "1", "0"]) {
        assert_eq!(row["row"], "");
        assert_eq!(row["usubjid"], "");
        assert_eq!(row["seq"], "");
        assert_eq!(row["rule_id"], "CORE-000013");
        assert_eq!(row["error_count"], expected_count);
    }
}
