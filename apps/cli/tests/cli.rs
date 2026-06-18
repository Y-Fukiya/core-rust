use assert_cmd::Command;
use predicates::prelude::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[test]
fn version_exits_successfully() {
    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");

    cmd.arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("core-rs"));
}

#[test]
fn validate_help_exits_successfully() {
    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");

    cmd.args(["validate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn validate_rejects_rules_and_exclude_rules_together() {
    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");

    cmd.args([
        "validate",
        "--rules",
        "CORE-TEST-0001",
        "--exclude-rules",
        "CORE-TEST-0002",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "--rules and --exclude-rules cannot be used together",
    ));
}

#[test]
fn validate_writes_skipped_result_for_missing_requested_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-TEST-0001.json"),
        r#"{
  "Core": { "Id": "CORE-TEST-0001", "Status": "Published" },
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
        "DOMAIN": ["AE"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");
    cmd.args([
        "validate",
        "--local-rules",
        rules_dir.to_str().expect("rules dir path"),
        "--dataset-path",
        dataset_path.to_str().expect("dataset path"),
        "--rules",
        "CORE-MISSING",
        "-o",
        output_dir.to_str().expect("output path"),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("validation completed: 1 result"));

    let report_json = fs::read_to_string(output_dir.join("report.json")).expect("read report json");
    let report_csv = fs::read_to_string(output_dir.join("report.csv")).expect("read report csv");

    assert!(report_json.contains("\"execution_status\": \"skipped\""));
    assert!(report_json.contains("\"skipped_reason\": \"rule_not_found\""));
    assert!(report_csv.contains("CORE-MISSING,skipped"));
    assert!(report_csv.contains("rule_not_found"));
}

#[test]
fn validate_honors_json_output_format() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-TEST-0001.json"),
        r#"{
  "Core": { "Id": "CORE-TEST-0001", "Status": "Published" },
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
        "DOMAIN": ["AE"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");
    cmd.args([
        "validate",
        "--local-rules",
        rules_dir.to_str().expect("rules dir path"),
        "--dataset-path",
        dataset_path.to_str().expect("dataset path"),
        "--output",
        output_dir.to_str().expect("output path"),
        "--output-format",
        "json",
        "--standard",
        "SDTMIG",
        "--standard-version",
        "3.4",
        "--log-level",
        "info",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("wrote").and(predicate::str::contains("report.json")))
    .stdout(predicate::str::contains("report.csv").not());

    let report_json = fs::read_to_string(output_dir.join("report.json")).expect("read report json");
    assert!(report_json.contains("\"standard\": \"SDTMIG\""));
    assert!(report_json.contains("\"standard_version\": \"3.4\""));
    assert!(report_json.contains("\"log_level\": \"info\""));
    assert!(!output_dir.join("report.csv").exists());
}

#[test]
fn validate_regulatory_fixture_writes_full_report_set() {
    let fixtures = fixture_root();
    let output_dir = tempdir().expect("tempdir");

    let mut cmd = Command::cargo_bin("core-rs").expect("core-rs binary");
    cmd.args([
        "validate",
        "--local-rules",
        fixtures
            .join("rules/regulatory")
            .to_str()
            .expect("rules dir path"),
        "--dataset-path",
        fixtures
            .join("datasets/regulatory/study_package.json")
            .to_str()
            .expect("dataset path"),
        "--define-xml",
        fixtures
            .join("cdisc/regulatory_define.xml")
            .to_str()
            .expect("define path"),
        "--ct",
        fixtures
            .join("cdisc/regulatory_ct.json")
            .to_str()
            .expect("ct path"),
        "--external-dictionary",
        fixtures
            .join("cdisc/regulatory_external_dictionary.csv")
            .to_str()
            .expect("dictionary path"),
        "--log-level",
        "info",
        "--output",
        output_dir.path().to_str().expect("output path"),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains(
        "validation completed: 17 result(s)",
    ))
    .stdout(predicate::str::contains("report.json"))
    .stdout(predicate::str::contains("report.csv"))
    .stdout(predicate::str::contains("validation.log"));

    let report_json = fs::read_to_string(output_dir.path().join("report.json"))
        .expect("read regulatory report json");
    assert!(report_json.contains("\"total_results\": 17"));
    assert!(report_json.contains("\"failed\": 9"));
    assert!(report_json.contains("\"external_dictionary_count\": 1"));
    assert!(report_json.contains("\"rule_id\": \"CORE-REG-0010\""));

    let actual_csv = fs::read_to_string(output_dir.path().join("report.csv"))
        .expect("read regulatory report csv");
    let expected_csv =
        fs::read_to_string(fixtures.join("expected/regulatory_validation_report.csv"))
            .expect("read expected regulatory report csv");
    assert_eq!(actual_csv, expected_csv);

    let log =
        fs::read_to_string(output_dir.path().join("validation.log")).expect("read validation log");
    assert!(log.contains("summary total_results=17 passed=8 failed=9 skipped=0 error_count=9"));
    assert!(log.contains("result rule_id=CORE-REG-0010 status=failed dataset=AE"));
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures")
}
