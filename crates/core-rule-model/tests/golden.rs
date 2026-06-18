use std::path::{Path, PathBuf};

use core_rule_model::load_rule_file;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[test]
fn golden_normalizes_json_rule() {
    assert_normalized_rule_golden(
        "rules/CORE-TEST-0001.json",
        "expected/normalized_CORE-TEST-0001.json",
    );
}

#[test]
fn golden_normalizes_yaml_rule() {
    assert_normalized_rule_golden(
        "rules/CORE-TEST-0002.yaml",
        "expected/normalized_CORE-TEST-0002.json",
    );
}

fn assert_normalized_rule_golden(rule_fixture: &str, expected_fixture: &str) {
    let rule_path = fixture_root().join(rule_fixture);
    let expected_path = fixture_root().join(expected_fixture);

    let actual = comparable_normalized_rule(&rule_path);
    let expected = read_json(&expected_path);

    assert_eq!(actual, expected);
}

fn comparable_normalized_rule(path: &Path) -> Value {
    let rule = load_rule_file(path).expect("load golden rule");
    let mut value = serde_json::to_value(rule).expect("serialize normalized rule");

    value
        .as_object_mut()
        .expect("normalized rule is an object")
        .remove("raw");

    value
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
