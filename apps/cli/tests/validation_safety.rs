use assert_cmd::Command;
use predicates::prelude::*;
use std::{fs, path::Path};
use tempfile::tempdir;

const RULE_ID: &str = "CORE-PILOT-0001";

fn write_rule(path: &Path, operator: &str) {
    fs::write(
        path,
        format!(
            r#"{{
  "Core": {{"Id": "{RULE_ID}", "Status": "Published"}},
  "Scope": {{"Domains": {{}}, "Classes": {{}}}},
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {{"name": "DOMAIN", "operator": "{operator}", "value": "AE"}},
  "Outcome": {{"Message": "Check DOMAIN"}}
}}"#
        ),
    )
    .unwrap();
}

fn write_dataset(path: &Path) {
    fs::write(
        path,
        r#"{"datasets":[{"filename":"ae.xpt","domain":"AE","records":{"DOMAIN":["AE"]}}]}"#,
    )
    .unwrap();
}

fn command(rules: &Path, data: &Path, out: &Path) -> Command {
    let mut cmd = Command::cargo_bin("core-rs").unwrap();
    cmd.arg("validate")
        .arg("--local-rules")
        .arg(rules)
        .arg("--dataset-path")
        .arg(data)
        .arg("--output")
        .arg(out);
    cmd
}

#[test]
fn empty_or_nested_only_rule_inputs_fail_before_writing_reports() {
    for input in ["empty", "nested", "unsupported"] {
        for strict in [false, true] {
            let dir = tempdir().unwrap();
            let rules = dir.path().join("rules");
            fs::create_dir(&rules).unwrap();
            if input == "nested" {
                fs::create_dir(rules.join("child")).unwrap();
                write_rule(&rules.join("child/rule.json"), "not_equal_to");
            } else if input == "unsupported" {
                fs::write(rules.join("notes.txt"), "not an executable rule").unwrap();
            }
            let data = dir.path().join("data.json");
            write_dataset(&data);
            let out = dir.path().join("out");
            let mut cmd = command(&rules, &data, &out);
            if strict {
                cmd.arg("--strict");
            }
            cmd.assert().failure().stderr(predicate::str::contains(
                "no executable rule files were loaded",
            ));
            assert!(!out.exists());
        }
    }
}

#[test]
fn strict_rejects_zero_results_after_exclusion_but_default_retains_report_only_policy() {
    let dir = tempdir().unwrap();
    let rule = dir.path().join("rule.json");
    let data = dir.path().join("data.json");
    write_rule(&rule, "not_equal_to");
    write_dataset(&data);

    command(&rule, &data, &dir.path().join("default"))
        .args(["--exclude-rules", RULE_ID])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 result(s)"));
    command(&rule, &data, &dir.path().join("strict"))
        .args(["--exclude-rules", RULE_ID, "--strict"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no validation results"));
}

#[test]
fn duplicate_rule_definitions_are_rejected_independent_of_order_and_selection() {
    for identical in [false, true] {
        let dir = tempdir().unwrap();
        let first = dir.path().join("first.json");
        let second = dir.path().join("second.json");
        let data = dir.path().join("data.json");
        write_rule(&first, "not_equal_to");
        write_rule(
            &second,
            if identical {
                "not_equal_to"
            } else {
                "equal_to"
            },
        );
        write_dataset(&data);
        for paths in [[&first, &second], [&second, &first]] {
            for filter in [None, Some("--rules"), Some("--exclude-rules")] {
                let out = dir.path().join("out");
                let mut cmd = Command::cargo_bin("core-rs").unwrap();
                cmd.args(["validate", "--strict", "--local-rules"])
                    .args(paths)
                    .arg("--dataset-path")
                    .arg(&data)
                    .arg("--output")
                    .arg(&out);
                if let Some(filter) = filter {
                    cmd.args([filter, RULE_ID]);
                }
                cmd.assert()
                    .failure()
                    .stderr(predicate::str::contains("duplicate rule id"))
                    .stderr(predicate::str::contains(first.to_string_lossy().as_ref()))
                    .stderr(predicate::str::contains(second.to_string_lossy().as_ref()));
                assert!(!out.exists());
            }
        }
    }
}

#[test]
fn reusing_report_directory_preserves_previous_bundle_and_fails() {
    let dir = tempdir().unwrap();
    let rule = dir.path().join("rule.json");
    let data = dir.path().join("data.json");
    let out = dir.path().join("out");
    write_rule(&rule, "equal_to");
    write_dataset(&data);
    command(&rule, &data, &out)
        .args(["--log-level", "info"])
        .assert()
        .success();
    let files = ["report.json", "report.csv", "validation.log"];
    let previous = files.map(|name| fs::read(out.join(name)).unwrap());

    write_rule(&rule, "not_equal_to");
    command(&rule, &data, &out)
        .args(["--strict", "--output-format", "json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already contains report"))
        .stdout(predicate::str::contains("validation completed").not());
    assert_eq!(
        files.map(|name| fs::read(out.join(name)).unwrap()),
        previous
    );

    command(&rule, &data, &dir.path().join("next-run"))
        .args(["--strict", "--output-format", "json"])
        .assert()
        .success();
}

#[test]
fn json_only_run_rejects_even_a_lone_old_csv_or_log() {
    for filename in ["report.csv", "validation.log"] {
        let dir = tempdir().unwrap();
        let rule = dir.path().join("rule.json");
        let data = dir.path().join("data.json");
        let out = dir.path().join("out");
        write_rule(&rule, "not_equal_to");
        write_dataset(&data);
        fs::create_dir(&out).unwrap();
        fs::write(out.join(filename), "previous run").unwrap();

        command(&rule, &data, &out)
            .args(["--strict", "--output-format", "json"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("already contains report"));
        assert!(!out.join("report.json").exists());
        assert_eq!(
            fs::read_to_string(out.join(filename)).unwrap(),
            "previous run"
        );
    }
}
