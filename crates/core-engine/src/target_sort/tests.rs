use core_data::{DatasetMetadata, DatasetSourceFormat, LoadedDataset};
use core_rule_model::{normalize_condition_value, normalize_rule};
use polars::prelude::{df, DataFrame, PolarsResult};
use serde_json::{json, Value};

use crate::{evaluate_condition_group, validate_rule};

fn dataset(domain: &str, frame: PolarsResult<DataFrame>) -> LoadedDataset {
    let filename = format!("{domain}.csv");
    LoadedDataset::new(
        DatasetMetadata {
            name: domain.to_owned(),
            domain: Some(domain.to_owned()),
            label: None,
            filename: filename.clone(),
            full_path: filename.into(),
            source_format: DatasetSourceFormat::Csv,
            variables: Vec::new(),
        },
        frame.expect("test frame"),
    )
}

fn strict_check(value: Value) -> Value {
    json!({
        "name": "--SEQ",
        "operator": "target_is_not_sorted_by",
        "strict_incremental_ordering": true,
        "within": "USUBJID",
        "value": value
    })
}

fn ascending_date_check() -> Value {
    strict_check(json!([{"name": "--STDTC", "sort_order": "asc", "null_position": "last"}]))
}

fn flagged_rows(dataset: &LoadedDataset, check: &Value) -> Vec<usize> {
    let condition = normalize_condition_value(check).expect("test condition");
    evaluate_condition_group(&condition, dataset)
        .expect("sort mask")
        .into_iter()
        .enumerate()
        .filter_map(|(row, flagged)| flagged.then_some(row + 1))
        .collect()
}

#[test]
fn strict_target_order_matches_core_000535_audit() {
    let sm = dataset(
        "SM",
        df!(
            "USUBJID" => ["001", "001", "001", "001", "002", "002", "002", "002", "002", "002",
                          "002", "002", "002", "003", "003", "003", "003"],
            "SMSEQ" => [Some(1), Some(2), Some(3), Some(4), Some(1), Some(2), Some(2), Some(4),
                        Some(5), Some(6), Some(7), Some(8), None, Some(1), Some(3), Some(4), Some(5)],
            "SMSTDTC" => ["2005-10", "2005-10-08", "2005-10-06", "2005-11", "2005-12", "2006-01-01",
                          "2006-01-02", "2006-01-03", "2006-01-04", "2006-01-05", "2006-02", "2006-03-15",
                          "2006-03-16", "2005-12-01", "2005-12-02", "2006-01-01", "2006-01-02"],
        ),
    );
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => ["123", "123", "456", "456", "456", "456"],
            "SESEQ" => [1, 2, 1, 2, 4, 3],
            "SESTDTC" => ["2006-06-01", "2006-06-03", "2006-05-01", "2006-05-03", "2006-05-31", "2006-06-13"],
        ),
    );
    let mut check = ascending_date_check();
    let rule = normalize_rule(json!({
        "Core": {"Id": "CORE-000535"},
        "Rule Type": "Record Data",
        "Sensitivity": "Record",
        "Check": {"all": [check.clone()]},
        "Outcome": {"Message": "--SEQ is not chronological", "Output Variables": ["--SEQ", "--STDTC"]}
    }))
    .expect("CORE-000535 rule");
    for (dataset, expected) in [
        (
            &sm,
            &[
                (2, "001", "2"),
                (3, "001", "3"),
                (7, "002", "2"),
                (15, "003", "3"),
                (16, "003", "4"),
                (17, "003", "5"),
            ][..],
        ),
        (&se, &[(5, "456", "4"), (6, "456", "3")][..]),
    ] {
        let result = validate_rule(&rule, dataset).expect("strict CORE-000535 results");
        let domain = &dataset.metadata().name;
        let variables = [format!("{domain}SEQ"), format!("{domain}STDTC")];
        let rows: Vec<_> = result
            .errors
            .iter()
            .map(|issue| {
                assert_eq!(issue.rule_id, "CORE-000535");
                assert_eq!(&issue.dataset, domain);
                assert_eq!(issue.domain.as_deref(), Some(domain.as_str()));
                assert_eq!(issue.variables.as_slice(), variables.as_slice());
                (
                    issue.row.unwrap(),
                    issue.usubjid.as_deref().unwrap(),
                    issue.seq.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(rows.as_slice(), expected);
    }

    check["strict_incremental_ordering"] = json!(false);
    assert_eq!(flagged_rows(&sm, &check), [1, 2, 3, 4]);
    check
        .as_object_mut()
        .unwrap()
        .remove("strict_incremental_ordering");
    assert_eq!(flagged_rows(&sm, &check), [1, 2, 3, 4]);
}

#[test]
fn strict_target_order_uses_each_key_and_keeps_source_order_for_ties() {
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => ["A"; 7],
            "SESEQ" => [6, 2, 4, 1, 5, 7, 2],
            "SESTDTC" => [None, Some("B"), Some("A"), Some("B"), Some(" \t"), Some(""), Some("B")],
            "SEKEY" => [Some(1), Some(1), None, None, Some(2), Some(1), Some(1)],
        ),
    );
    let check = strict_check(json!([
        {"name": "--STDTC", "sort_order": "DeSc"},
        {"name": "--KEY", "sort_order": "desc", "null_position": "FIRST"}
    ]));
    assert_eq!(flagged_rows(&se, &check), [7]);
}

#[test]
fn strict_target_order_keeps_null_and_blank_groups_and_counts_missing_targets() {
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => [Some("A"), None, Some(""), None, Some("A"), None, Some(""), Some("A"), None],
            "TYPE" => ["X", "X", "X", "X", "Y", "X", "X", "X", "X"],
            "SESEQ" => [Some(" 02 "), None, Some("2"), Some(""), Some("2"), Some("bad"), Some("1"), Some(" \t"), Some("04")],
            "SESTDTC" => ["2000-03", "2000-02", "2000-03", "2000-01", "2000-01", "2000-03", "2000-01", "2000-01", "2000-04"],
        ),
    );
    let mut check = ascending_date_check();
    check["within"] = json!(["USUBJID", "TYPE"]);
    assert_eq!(flagged_rows(&se, &check), [5, 6]);
}

#[test]
fn strict_target_order_sorts_raw_text_and_boolean_keys() {
    let text = dataset(
        "SE",
        df!(
            "USUBJID" => ["A"; 5],
            "SESEQ" => [4, 2, 3, 5, 1],
            "SESTDTC" => ["2", "10", "1a", "bad", " A"],
        ),
    );
    let boolean = dataset(
        "SE",
        df!(
            "USUBJID" => ["A"; 4],
            "SESEQ" => [3, 1, 3, 2],
            "SESTDTC" => [true, false, true, false],
        ),
    );
    assert!(flagged_rows(&text, &ascending_date_check()).is_empty());
    assert_eq!(flagged_rows(&boolean, &ascending_date_check()), [3]);
}

#[test]
fn strict_target_order_places_nan_keys_with_nulls_and_exempts_nan_targets() {
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => ["A"; 5],
            "SESEQ" => [4.0, f64::NAN, 5.0, 1.0, 3.0],
            "SESTDTC" => [Some(f64::NAN), Some(5.0), None, Some(f64::NEG_INFINITY), Some(f64::INFINITY)],
        ),
    );
    assert!(flagged_rows(&se, &ascending_date_check()).is_empty());
}

#[test]
fn strict_target_order_can_extract_the_sequence_before_comparing_ranks() {
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => ["A"; 4],
            "SESEQ" => ["MIDS01", "missing", "MIDS03", "MIDS07"],
            "SESTDTC" => ["2000-01", "2000-02", "2000-03", "2000-04"],
        ),
    );
    let mut check = ascending_date_check();
    check["regex"] = json!(r"MIDS(\d+)");
    assert_eq!(flagged_rows(&se, &check), [4]);
}

#[test]
fn strict_target_order_keeps_the_decimal_point_for_float_target_regexes() {
    let se = dataset(
        "SE",
        df!(
            "USUBJID" => ["A", "A"],
            "SESEQ" => [2.0, 2.0],
            "SESTDTC" => ["2000-01", "2000-02"],
        ),
    );
    let mut check = ascending_date_check();
    check["regex"] = json!(r"^([0-9]+)\.0$");
    assert_eq!(flagged_rows(&se, &check), [1]);
}
