use std::{
    fs,
    path::{Path, PathBuf},
};

use core_api::{run_validation, ValidateRequest};
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct PythonCompatCase {
    name: String,
    #[serde(default)]
    description: Option<String>,
    rule_paths: Vec<PathBuf>,
    dataset_paths: Vec<PathBuf>,
    #[serde(default)]
    define_xml_paths: Vec<PathBuf>,
    #[serde(default)]
    ct_paths: Vec<PathBuf>,
    #[serde(default)]
    include_rules: Vec<String>,
    #[serde(default)]
    exclude_rules: Vec<String>,
    #[serde(default)]
    standard: Option<String>,
    #[serde(default)]
    standard_version: Option<String>,
    expected_path: PathBuf,
}

#[test]
fn python_compat_cases_match_stored_expected_outputs() {
    let fixtures = fixture_root();
    let mut case_paths = fs::read_dir(fixtures.join("python_compat/cases"))
        .expect("read python compat cases")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("read case entries")
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    case_paths.sort();

    assert!(
        !case_paths.is_empty(),
        "expected at least one Python compat case"
    );

    for case_path in case_paths {
        let case = read_case(&case_path);
        let request = ValidateRequest {
            rule_paths: absolute_paths(&fixtures, &case.rule_paths),
            dataset_paths: absolute_paths(&fixtures, &case.dataset_paths),
            define_xml_paths: absolute_paths(&fixtures, &case.define_xml_paths),
            ct_paths: absolute_paths(&fixtures, &case.ct_paths),
            include_rules: case.include_rules,
            exclude_rules: case.exclude_rules,
            standard: case.standard,
            standard_version: case.standard_version,
            ..Default::default()
        };
        let outcome = run_validation(request)
            .unwrap_or_else(|source| panic!("case {} failed to run: {source}", case.name));
        let actual = comparable_validation_output(&serde_json::to_value(outcome.results).unwrap());
        let expected = read_json(&fixtures.join(&case.expected_path));

        assert_eq!(
            actual,
            expected,
            "Python compat case {} did not match{}",
            case.name,
            case.description
                .as_deref()
                .map(|description| format!(": {description}"))
                .unwrap_or_default()
        );
    }
}

fn absolute_paths(root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().map(|path| root.join(path)).collect()
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

fn read_case(path: &Path) -> PythonCompatCase {
    serde_json::from_str(&fs::read_to_string(path).expect("read python compat case"))
        .expect("parse python compat case")
}

fn read_json(path: &Path) -> Value {
    let source = fs::read_to_string(path).expect("read python compat expected output");
    serde_json::from_str(&source).expect("parse python compat expected output")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures")
}
