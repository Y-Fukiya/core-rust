use std::path::{Path, PathBuf};

use core_api::{run_validation, ValidateRequest};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

#[test]
fn golden_validates_supported_rules_against_csv_fixture() {
    let fixtures = fixture_root();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![fixtures.join("rules")],
        dataset_paths: vec![fixtures.join("datasets/AE.csv")],
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
    })
    .expect("run golden validation");

    let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
    let expected = read_json(&fixtures.join("expected/later_validation_output.json"));

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
