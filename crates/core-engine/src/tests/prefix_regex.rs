use super::*;
use pretty_assertions::assert_eq;

fn evaluate_prefix(values: serde_json::Value, prefix: usize, pattern: &str) -> Vec<bool> {
    let dir = tempdir().unwrap();
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"datasets": [{"filename": "ae.json", "domain": "AE",
            "records": {"TERM": values}}]}))
        .unwrap(),
    )
    .unwrap();
    let datasets = load_dataset_package_json(&path).unwrap();
    evaluate_condition(
        &condition_with_options(
            "TERM",
            Operator::from_name("prefix_matches_regex"),
            literal(pattern),
            json!({"prefix": prefix}).as_object().unwrap().clone(),
        ),
        &datasets[0],
    )
    .expect("positive prefix regex is supported")
}

#[test]
fn prefix_regex_searches_only_the_requested_prefix() {
    assert_eq!(
        evaluate_prefix(json!(["APxx", "apyy", "xxAP", "A", null]), 2, "(AP|ap)"),
        vec![true, true, false, false, false]
    );
    assert_eq!(
        evaluate_prefix(json!(["xArest", "xxArest"]), 2, "A"),
        vec![true, false]
    );
}

#[test]
fn prefix_regex_counts_characters_not_utf8_bytes() {
    assert_eq!(
        evaluate_prefix(json!(["\u{00e9}A-tail", "\u{00e9}xA-tail"]), 2, "A"),
        vec![true, false]
    );
}

#[test]
fn prefix_regex_preserves_empty_string_but_excludes_null() {
    assert_eq!(
        evaluate_prefix(json!(["", null, "A"]), 2, "^$"),
        vec![true, false, false]
    );
    assert_eq!(
        evaluate_prefix(json!(["", null, "A"]), 0, "^$"),
        vec![true, false, true]
    );
    assert_eq!(
        evaluate_prefix(json!(["A", "AB"]), 100, "^A$"),
        vec![true, false]
    );
}

#[test]
fn prefix_regex_converts_numeric_values() {
    assert_eq!(
        evaluate_prefix(json!([12.0, 1.25, null]), 2, "^12$"),
        vec![true, false, false]
    );
}

#[test]
fn prefix_regex_rejects_invalid_configuration() {
    let dataset = test_dataset();
    for options in [json!({}), json!({"prefix": -1}), json!({"prefix": "two"})] {
        let error = evaluate_condition(
            &condition_with_options(
                "TERM",
                Operator::from_name("prefix_matches_regex"),
                literal("A"),
                options.as_object().unwrap().clone(),
            ),
            &dataset,
        )
        .expect_err("invalid prefix");
        assert!(matches!(error, EngineError::MissingComparator { .. }));
    }
    assert!(evaluate_condition(
        &condition_with_options(
            "TERM",
            Operator::from_name("prefix_matches_regex"),
            literal("["),
            json!({"prefix": 2}).as_object().unwrap().clone()
        ),
        &dataset
    )
    .is_err());
}

#[test]
fn existing_negative_prefix_empty_value_policy_is_unchanged() {
    assert_eq!(
        evaluate_condition(
            &condition_with_options(
                "TERM",
                Operator::NotPrefixMatchesRegex,
                literal("^$"),
                json!({"prefix": 2}).as_object().unwrap().clone()
            ),
            &test_dataset()
        )
        .unwrap(),
        vec![true, true, false, false]
    );
}
