use std::{
    fs,
    path::{Path, PathBuf},
};

use core_api::{run_validation, ValidateRequest};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tempfile::tempdir;

#[test]
fn golden_validates_supported_rules_against_csv_fixture() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![
            fixtures.join("rules/CORE-TEST-0001.json"),
            fixtures.join("rules/CORE-TEST-0002.yaml"),
        ],
        dataset_paths: vec![fixtures.join("datasets/AE.csv")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/later_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_validates_record_rule_pass_and_failure_cases() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/CORE-TEST-0003.json")],
        dataset_paths: vec![
            fixtures.join("datasets/AE.csv"),
            fixtures.join("datasets/mixed/AE.csv"),
        ],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run record pass/fail golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/record_pass_fail_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_validates_dataset_package_json_input() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/CORE-TEST-0003.json")],
        dataset_paths: vec![fixtures.join("datasets/dataset_package.json")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run dataset package golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/dataset_package_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_validates_dataset_sensitivity_rule() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/CORE-TEST-0004.json")],
        dataset_paths: vec![fixtures.join("datasets/mixed/AE.csv")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run dataset sensitivity golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/dataset_sensitivity_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_records_skipped_results() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/CORE-TEST-0003.json")],
        dataset_paths: vec![fixtures.join("datasets/AE.csv")],
        include_rules: vec!["CORE-MISSING".to_owned()],
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run skipped golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/skipped_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_writes_json_and_csv_reports() {
    let fixtures = fixture_root();
    let output_dir = tempdir().expect("tempdir");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/CORE-TEST-0003.json")],
        dataset_paths: vec![fixtures.join("datasets/mixed/AE.csv")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: Some(output_dir.path().to_path_buf()),
        ..Default::default()
    })
    .expect("run report golden validation");

    let reports = outcome.reports.expect("reports");
    let report_json = read_json(reports.json.as_ref().expect("json report"));
    assert_eq!(report_json["metadata"]["schema_version"], "1.0");
    assert_eq!(report_json["metadata"]["engine"], "core-rs");
    assert_eq!(report_json["summary"]["total_results"], 1);
    assert_eq!(report_json["summary"]["failed"], 1);
    let actual_json = comparable_validation_output(&report_json["results"]);
    let expected_json = read_json(&fixtures.join("expected/report_validation_report.json"));
    assert_eq!(actual_json, expected_json);

    let actual_csv =
        fs::read_to_string(reports.csv.as_ref().expect("csv report")).expect("read report csv");
    let expected_csv = fs::read_to_string(fixtures.join("expected/report_validation_report.csv"))
        .expect("read expected csv");
    assert_eq!(actual_csv, expected_csv);
}

#[test]
fn golden_validates_integrated_study_package_with_define_xml_and_ct() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/integrated")],
        dataset_paths: vec![fixtures.join("datasets/integrated/study_package.json")],
        define_xml_paths: vec![fixtures.join("cdisc/integrated_define.xml")],
        ct_paths: vec![fixtures.join("cdisc/integrated_ct.json")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run integrated golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/integrated_validation_output.json"));

    assert_eq!(actual, expected);
}

#[test]
fn golden_validates_sdtm_adam_like_study_package() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules/sdtm_adam")],
        dataset_paths: vec![fixtures.join("datasets/sdtm_adam/study_package.json")],
        define_xml_paths: vec![fixtures.join("cdisc/sdtm_adam_define.xml")],
        ct_paths: vec![fixtures.join("cdisc/sdtm_adam_ct.json")],
        external_dictionary_paths: vec![fixtures.join("cdisc/sdtm_adam_external_dictionary.json")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run SDTM/ADaM-like golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected =
        read_json(&fixtures.join("python_compat/expected/sdtm_adam_full_study_package.json"));

    assert_eq!(actual, expected);
}

fn comparable_validation_output(results: &Value) -> Value {
    let results = results.as_array().expect("results are an array");
    let comparable_results = results
        .iter()
        .map(|result| {
            let errors = result["errors"]
                .as_array()
                .expect("errors are an array")
                .iter()
                .map(|error| {
                    json!({
                        "rule_id": error["rule_id"],
                        "dataset": error["dataset"],
                        "domain": error["domain"],
                        "row": error["row"],
                        "variables": error["variables"],
                        "message": error["message"],
                    })
                })
                .collect::<Vec<_>>();

            json!({
                "rule_id": result["rule_id"],
                "execution_status": result["execution_status"],
                "skipped_reason": result.get("skipped_reason").cloned().unwrap_or(Value::Null),
                "dataset": result["dataset"],
                "domain": result["domain"],
                "message": result["message"],
                "error_count": result["error_count"],
                "errors": errors,
            })
        })
        .collect::<Vec<_>>();

    json!({ "results": comparable_results })
}

fn read_json(path: &Path) -> Value {
    let source = std::fs::read_to_string(path).expect("read golden fixture");
    serde_json::from_str(&source).expect("parse golden fixture")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures")
}
