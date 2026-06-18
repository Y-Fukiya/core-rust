use std::{
    collections::{BTreeMap, BTreeSet},
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

#[derive(Debug, Deserialize)]
struct CoverageManifest {
    minimum_case_count: usize,
    minimum_rule_count: usize,
    required_cases: Vec<String>,
    required_domains: Vec<String>,
    required_rule_ids: Vec<String>,
    expected_result_counts: BTreeMap<String, ExpectedResultCounts>,
    required_capabilities: Vec<CoverageCapability>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExpectedResultCounts {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    errors: usize,
}

#[derive(Debug, Deserialize)]
struct CoverageCapability {
    name: String,
    evidence: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct IdentityManifest {
    expected_issue_identities: BTreeMap<String, Value>,
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
        case_paths.len() >= 7,
        "expected at least seven Python compat cases"
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
fn python_compat_cases_match_issue_identity_manifest() {
    let fixtures = fixture_root();
    let manifest: IdentityManifest = serde_json::from_str(
        &fs::read_to_string(fixtures.join("python_compat/identity_manifest.json"))
            .expect("read identity manifest"),
    )
    .expect("parse identity manifest");
    let cases = read_cases_by_name(&fixtures);

    for (case_name, expected) in &manifest.expected_issue_identities {
        let case = cases
            .get(case_name)
            .unwrap_or_else(|| panic!("identity manifest references unknown case {case_name}"));
        let actual =
            issue_identity_output(&serde_json::to_value(run_case(&fixtures, case)).unwrap());
        assert_eq!(
            &actual, expected,
            "issue identity manifest did not match {case_name}"
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
    assert!(case_names.contains("regulatory_sdtmig_filter"));
    assert!(case_names.contains("regulatory_include_missing_rules"));

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

    let regulatory_filters = [
        read_case(&fixtures.join("python_compat/cases/regulatory_adamig_filter.json")),
        read_case(&fixtures.join("python_compat/cases/regulatory_sdtmig_filter.json")),
    ];
    assert!(
        regulatory_filters.iter().all(|case| case.standard.is_some()
            && case.standard_version.is_some()
            && case.external_dictionary_paths.len() == 1),
        "expected regulatory standard filter cases to include standard, version, and dictionary inputs"
    );

    let regulatory_include =
        read_case(&fixtures.join("python_compat/cases/regulatory_include_missing_rules.json"));
    assert!(
        regulatory_include
            .include_rules
            .iter()
            .any(|rule| rule == "CORE-MISSING"),
        "expected regulatory include case to cover missing requested rules"
    );
}

#[test]
fn python_compat_coverage_manifest_matches_fixture_matrix() {
    let fixtures = fixture_root();
    let manifest: CoverageManifest = serde_json::from_str(
        &fs::read_to_string(fixtures.join("python_compat/coverage_manifest.json"))
            .expect("read coverage manifest"),
    )
    .expect("parse coverage manifest");
    let cases = read_cases_by_name(&fixtures);

    assert!(
        cases.len() >= manifest.minimum_case_count,
        "expected at least {} compatibility cases",
        manifest.minimum_case_count
    );
    for required_case in &manifest.required_cases {
        assert!(
            cases.contains_key(required_case),
            "coverage manifest requires missing case {required_case}"
        );
        assert!(
            manifest.expected_result_counts.contains_key(required_case),
            "coverage manifest requires result counts for {required_case}"
        );
    }

    for (case_name, expected_counts) in &manifest.expected_result_counts {
        let case = cases
            .get(case_name)
            .unwrap_or_else(|| panic!("expected result count for unknown case {case_name}"));
        let expected = read_json(&fixtures.join(&case.expected_path));
        assert_eq!(
            &result_counts(&expected),
            expected_counts,
            "coverage manifest result counts did not match {case_name}"
        );
    }

    let covered_domains = cases
        .values()
        .flat_map(|case| case.dataset_paths.iter())
        .flat_map(|path| dataset_domains(&fixtures.join(path)))
        .collect::<BTreeSet<_>>();
    for domain in &manifest.required_domains {
        assert!(
            covered_domains.contains(domain),
            "coverage manifest requires uncovered domain {domain}"
        );
    }

    let mut covered_rule_ids = BTreeSet::new();
    for case in cases.values() {
        for path in &case.rule_paths {
            collect_rule_ids(&fixtures.join(path), &mut covered_rule_ids);
        }
    }
    assert!(
        covered_rule_ids.len() >= manifest.minimum_rule_count,
        "expected at least {} covered rules",
        manifest.minimum_rule_count
    );
    for rule_id in &manifest.required_rule_ids {
        assert!(
            covered_rule_ids.contains(rule_id),
            "coverage manifest requires uncovered rule {rule_id}"
        );
    }

    let mut capability_names = BTreeSet::new();
    for capability in &manifest.required_capabilities {
        assert!(
            capability_names.insert(capability.name.as_str()),
            "duplicate capability {}",
            capability.name
        );
        assert!(
            !capability.evidence.is_empty(),
            "capability {} needs at least one evidence path",
            capability.name
        );
        for evidence in &capability.evidence {
            assert!(
                fixtures.join(evidence).exists(),
                "capability {} evidence path does not exist: {}",
                capability.name,
                evidence.display()
            );
        }
    }
}

fn absolute_paths(root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().map(|path| root.join(path)).collect()
}

fn run_case(root: &Path, case: &PythonCompatCase) -> Vec<core_engine::RuleValidationResult> {
    let request = ValidateRequest {
        rule_paths: absolute_paths(root, &case.rule_paths),
        dataset_paths: absolute_paths(root, &case.dataset_paths),
        define_xml_paths: absolute_paths(root, &case.define_xml_paths),
        ct_paths: absolute_paths(root, &case.ct_paths),
        external_dictionary_paths: absolute_paths(root, &case.external_dictionary_paths),
        include_rules: case.include_rules.clone(),
        exclude_rules: case.exclude_rules.clone(),
        standard: case.standard.clone(),
        standard_version: case.standard_version.clone(),
        ..Default::default()
    };
    run_validation(request)
        .unwrap_or_else(|source| panic!("case {} failed to run: {source}", case.name))
        .results
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

fn issue_identity_output(results: &Value) -> Value {
    let results = results.as_array().expect("results are an array");
    Value::Array(
        results
            .iter()
            .filter(|result| result["execution_status"] != "passed")
            .map(|result| {
                let identities = result["errors"]
                    .as_array()
                    .expect("errors are an array")
                    .iter()
                    .map(|error| {
                        json!({
                            "row": error["row"],
                            "usubjid": error.get("usubjid").cloned().unwrap_or(Value::Null),
                            "seq": error.get("seq").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect::<Vec<_>>();

                json!({
                    "rule_id": result["rule_id"],
                    "execution_status": result["execution_status"],
                    "skipped_reason": result.get("skipped_reason").cloned().unwrap_or(Value::Null),
                    "dataset": result["dataset"],
                    "error_count": result["error_count"],
                    "identities": identities,
                })
            })
            .collect(),
    )
}

fn read_case(path: &Path) -> PythonCompatCase {
    serde_json::from_str(&fs::read_to_string(path).expect("read python compat case"))
        .expect("parse python compat case")
}

fn read_cases_by_name(root: &Path) -> BTreeMap<String, PythonCompatCase> {
    fs::read_dir(root.join("python_compat/cases"))
        .expect("read python compat cases")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("read case entries")
        .into_iter()
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .map(|entry| {
            let case = read_case(&entry.path());
            (case.name.clone(), case)
        })
        .collect()
}

fn read_json(path: &Path) -> Value {
    let source = fs::read_to_string(path).expect("read python compat expected output");
    serde_json::from_str(&source).expect("parse python compat expected output")
}

fn result_counts(expected: &Value) -> ExpectedResultCounts {
    let results = expected["results"].as_array().expect("results array");
    ExpectedResultCounts {
        total: results.len(),
        passed: results
            .iter()
            .filter(|result| result["execution_status"] == "passed")
            .count(),
        failed: results
            .iter()
            .filter(|result| result["execution_status"] == "failed")
            .count(),
        skipped: results
            .iter()
            .filter(|result| result["execution_status"] == "skipped")
            .count(),
        errors: results
            .iter()
            .map(|result| result["error_count"].as_u64().unwrap_or_default() as usize)
            .sum(),
    }
}

fn dataset_domains(path: &Path) -> Vec<String> {
    read_json(path)["datasets"]
        .as_array()
        .expect("datasets array")
        .iter()
        .filter_map(|dataset| dataset["domain"].as_str())
        .map(str::to_owned)
        .collect()
}

fn collect_rule_ids(path: &Path, ids: &mut BTreeSet<String>) {
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .expect("read rule dir")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("read rule dir entries");
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            collect_rule_ids(&entry.path(), ids);
        }
        return;
    }

    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return;
    }

    let rule = read_json(path);
    if let Some(rule_id) = rule["Core"]["Id"]
        .as_str()
        .or_else(|| rule["core_id"].as_str())
    {
        ids.insert(rule_id.to_owned());
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures")
}
