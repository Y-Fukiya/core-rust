use std::{
    collections::BTreeSet,
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
    external_dictionary_paths: Vec<PathBuf>,
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
        case_paths.len() >= 5,
        "expected at least five Python compat cases"
    );

    for case_path in case_paths {
        let case = read_case(&case_path);
        let request = ValidateRequest {
            rule_paths: absolute_paths(&fixtures, &case.rule_paths),
            dataset_paths: absolute_paths(&fixtures, &case.dataset_paths),
            define_xml_paths: absolute_paths(&fixtures, &case.define_xml_paths),
            ct_paths: absolute_paths(&fixtures, &case.ct_paths),
            external_dictionary_paths: absolute_paths(&fixtures, &case.external_dictionary_paths),
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

#[test]
fn python_compat_matrix_covers_sdtm_adam_study_shapes() {
    let fixtures = fixture_root();
    let case_names = fs::read_dir(fixtures.join("python_compat/cases"))
        .expect("read python compat cases")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("read case entries")
        .into_iter()
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .map(|entry| read_case(&entry.path()).name)
        .collect::<BTreeSet<_>>();

    assert!(case_names.contains("integrated_study_package"));
    assert!(case_names.contains("sdtm_adam_full_study_package"));
    assert!(case_names.contains("sdtm_adam_sdtmig_filter"));
    assert!(case_names.contains("regulatory_full_study_package"));
    assert!(case_names.contains("regulatory_adamig_filter"));

    let package = read_json(&fixtures.join("datasets/sdtm_adam/study_package.json"));
    let domains = package["datasets"]
        .as_array()
        .expect("datasets array")
        .iter()
        .filter_map(|dataset| dataset["domain"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        domains,
        BTreeSet::from(["ADAE", "ADSL", "AE", "CM", "DM", "SUPPAE"])
    );

    let rule_count = fs::read_dir(fixtures.join("rules/sdtm_adam"))
        .expect("read SDTM/ADaM rules")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count();
    assert!(
        rule_count >= 7,
        "expected at least seven SDTM/ADaM compatibility rules"
    );

    let regulatory_package = read_json(&fixtures.join("datasets/regulatory/study_package.json"));
    let regulatory_domains = regulatory_package["datasets"]
        .as_array()
        .expect("regulatory datasets array")
        .iter()
        .filter_map(|dataset| dataset["domain"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        regulatory_domains,
        BTreeSet::from(["ADLB", "ADSL", "AE", "DM", "EX", "LB", "RELREC", "VS"])
    );

    let regulatory_rule_count = fs::read_dir(fixtures.join("rules/regulatory"))
        .expect("read regulatory rules")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count();
    assert!(
        regulatory_rule_count >= 10,
        "expected at least ten regulatory compatibility rules"
    );

    let regulatory_full =
        read_case(&fixtures.join("python_compat/cases/regulatory_full_study_package.json"));
    assert!(
        regulatory_full
            .external_dictionary_paths
            .iter()
            .any(|path| path.extension().and_then(|value| value.to_str()) == Some("csv")),
        "expected regulatory compat case to cover CSV external dictionaries"
    );
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
