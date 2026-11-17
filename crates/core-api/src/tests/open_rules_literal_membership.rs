use std::fs;

use core_data::load_dataset_file;
use core_engine::{evaluate_condition, validate_rule, ExecutionStatus};
use core_rule_model::{load_rule_file, Condition, ConditionGroup, Operator, ValueExpr};
use serde_json::json;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

const Y_N_RULES: [(&str, &str); 9] = [
    ("CORE-000123", "AESCAN"),
    ("CORE-000124", "AESCONG"),
    ("CORE-000125", "AESDISAB"),
    ("CORE-000126", "AESDTH"),
    ("CORE-000127", "AESHOSP"),
    ("CORE-000128", "AESLIFE"),
    ("CORE-000129", "AESOD"),
    ("CORE-000130", "AESMIE"),
    ("CORE-000131", "AECONTRT"),
];

#[test]
fn joined_literal_membership_does_not_change_match_dataset_prefix() {
    let dir = tempdir().expect("tempdir");
    let dataset_path = dir.path().join("datasets.json");
    fs::write(
        &dataset_path,
        serde_json::to_vec(&json!({"datasets": [
            {"filename": "ae.xpt", "domain": "AE", "records": {
                "USUBJID": ["S1"], "AESEQ": [1], "X": ["left"]
            }},
            {"filename": "dm.xpt", "domain": "DM", "records": {
                "USUBJID": ["S1"], "X": ["Y"], "M": ["N"]
            }}
        ]}))
        .unwrap(),
    )
    .expect("write datasets");
    let rule_path = dir.path().join("rule.json");
    fs::write(
        &rule_path,
        serde_json::to_vec(&json!({
            "Core": {"Id": "CORE-LITERAL-JOIN"},
            "Scope": {"Domains": {"Include": ["AE"]}},
            "Rule Type": "Record Data",
            "Sensitivity": "Record",
            "Match Datasets": [{"Name": "DM", "Join Type": "left", "Keys": ["USUBJID"]}],
            "Check": {
                "name": "X.DM", "operator": "is_not_contained_by",
                "value": ["DM.M"], "value_is_literal": true
            },
            "Outcome": {"Message": "Joined value is not an allowed literal"}
        }))
        .unwrap(),
    )
    .expect("write rule");
    for oracle_compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rule_path.clone()],
            dataset_paths: vec![dataset_path.clone()],
            open_rules_oracle_compat: oracle_compat,
            ..Default::default()
        })
        .expect("run joined validation");
        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.skipped_reason, None);
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        let issue = &result.errors[0];
        assert_eq!(issue.rule_id, "CORE-LITERAL-JOIN");
        assert_eq!(issue.dataset, "AE");
        assert_eq!(issue.domain.as_deref(), Some("AE"));
        assert_eq!(issue.row, Some(1));
        assert_eq!(issue.variables, vec!["X.DM"]);
        assert_eq!(issue.usubjid.as_deref(), Some("S1"));
        assert_eq!(issue.seq.as_deref(), Some("1"));
    }
}

#[test]
fn literal_membership_lists_do_not_resolve_column_names() {
    let dir = tempdir().expect("tempdir");
    let dataset_path = dir.path().join("ae.json");
    fs::write(
        &dataset_path,
        serde_json::to_vec(&json!({"datasets": [{
            "filename": "ae.xpt",
            "domain": "AE",
            "records": {
                "AESCAN": ["Y", "N", "$OTHER", "OTHER", "Z", "y", "n", ""],
                "OTHER": ["Z", "Z", "Z", "Z", "Z", "Z", "Z", "Z"],
            },
        }]}))
        .unwrap(),
    )
    .expect("write dataset");
    for (operator, matching_rows) in [
        ("is_contained_by", vec![1, 2, 3, 4]),
        ("is_not_contained_by", vec![5, 6, 7, 8]),
        ("is_contained_by_case_insensitive", vec![1, 2, 3, 4, 6, 7]),
        ("is_not_contained_by_case_insensitive", vec![5, 8]),
    ] {
        for values in [json!(["Y", "N", "$OTHER", "OTHER"]), json!([])] {
            let rule_path = dir.path().join("rule.json");
            fs::write(
                &rule_path,
                serde_json::to_vec(&json!({
                    "Core": {"Id": "CORE-LITERAL-MEMBERSHIP"},
                    "Scope": {},
                    "Sensitivity": "Record",
                    "Rule Type": "Record Data",
                    "Check": {
                        "name": "AESCAN",
                        "operator": operator,
                        "value": values,
                        "value_is_literal": true,
                    },
                    "Outcome": {"Message": "Membership match"},
                }))
                .unwrap(),
            )
            .expect("write rule");
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rule_path],
                dataset_paths: vec![dataset_path.clone()],
                ..Default::default()
            })
            .expect("run validation");
            assert_eq!(outcome.results.len(), 1);
            let result = &outcome.results[0];
            assert_eq!(result.skipped_reason, None, "{operator}");
            let expected = if values.as_array().unwrap().is_empty() {
                if operator.starts_with("is_not_") {
                    (1..=8).collect()
                } else {
                    vec![]
                }
            } else {
                matching_rows.clone()
            };
            assert_eq!(result.error_count, expected.len(), "{operator} {values}");
            assert_eq!(
                result
                    .errors
                    .iter()
                    .filter_map(|e| e.row)
                    .collect::<Vec<_>>(),
                expected,
                "{operator} {values}",
            );
        }
    }
}

#[test]
fn direct_engine_and_native_rule_literal_arrays_are_scoped_to_containment() {
    let dir = tempdir().expect("tempdir");
    let dataset_path = dir.path().join("ae.json");
    fs::write(
        &dataset_path,
        serde_json::to_vec(&json!({"datasets": [{
            "filename": "ae.xpt", "domain": "AE", "records": {
                "CODE": ["Y", "N", "y", "n", "Z", "$OTHER", "OTHER", "DM.M", "", null, "[\"Y\",\"N\"]"],
                "OTHER": ["Z", "Z", "Z", "Z", "Z", "Z", "Z", "Z", "Z", "Z", "Z"],
                "NUMBER": [7, 8, 7, 8, 7, 8, 7, 8, 7, 8, null]
            }
        }]}))
        .unwrap(),
    )
    .expect("write dataset");
    let datasets = load_dataset_file(&dataset_path).expect("load dataset");
    let dataset = &datasets[0];
    let mut cases = Vec::new();
    for (operator, case_insensitive, negative) in [
        (Operator::IsContainedBy, false, false),
        (Operator::IsNotContainedBy, false, true),
        (Operator::IsContainedByCaseInsensitive, true, false),
        (Operator::IsNotContainedByCaseInsensitive, true, true),
    ] {
        for (target, values, matching_rows) in [
            (
                "CODE",
                json!(["Y", "N", "$OTHER", "OTHER", "DM.M"]),
                if case_insensitive {
                    vec![1, 2, 3, 4, 6, 7, 8]
                } else {
                    vec![1, 2, 6, 7, 8]
                },
            ),
            ("CODE", json!([]), vec![]),
            ("CODE", json!([null]), vec![10]),
            ("NUMBER", json!([7]), vec![1, 3, 5, 7, 9]),
        ] {
            let expected = (1..=11)
                .filter(|row| matching_rows.contains(row) != negative)
                .collect::<Vec<_>>();
            for comparator in [
                ValueExpr::Literal(values.clone()),
                ValueExpr::List(values.as_array().unwrap().clone()),
            ] {
                cases.push((target, operator.clone(), comparator, expected.clone()));
            }
        }
    }
    for (operator, literal_rows, list_rows) in [
        (Operator::EqualTo, vec![11], vec![1, 2]),
        (
            Operator::PrefixIsNotContainedBy,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            vec![3, 4, 5, 6, 7, 8, 11],
        ),
        (
            Operator::SuffixIsNotContainedBy,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            vec![3, 4, 5, 6, 7, 8, 11],
        ),
        (Operator::ContainsAll, vec![11], vec![]),
    ] {
        cases.push((
            "CODE",
            operator.clone(),
            ValueExpr::Literal(json!(["Y", "N"])),
            literal_rows,
        ));
        cases.push((
            "CODE",
            operator,
            ValueExpr::List(vec![json!("Y"), json!("N")]),
            list_rows,
        ));
    }
    let rule_path = dir.path().join("native.json");
    for (target, operator, comparator, expected_rows) in cases {
        let mut condition = Condition {
            target: Some(target.to_owned()),
            operator,
            comparator,
            options: Default::default(),
        };
        condition
            .options
            .extra
            .insert("prefix".to_owned(), json!(9));
        condition
            .options
            .extra
            .insert("suffix".to_owned(), json!(9));
        let expected_mask = (1..=11)
            .map(|row| expected_rows.contains(&row))
            .collect::<Vec<_>>();
        assert_eq!(
            evaluate_condition(&condition, dataset).expect("direct engine evaluation"),
            expected_mask,
            "{condition:?}",
        );
        fs::write(
            &rule_path,
            serde_json::to_vec(&json!({
                "core_id": "NATIVE-LITERAL-MEMBERSHIP",
                "rule_type": "Record Data", "sensitivity": "Record",
                "conditions": {"leaf": condition}, "actions": []
            }))
            .unwrap(),
        )
        .expect("write native rule");
        let rule = load_rule_file(&rule_path).expect("load native rule");
        assert_eq!(rule.conditions, ConditionGroup::Leaf(condition));
        let before = serde_json::to_value(&rule).expect("serialize rule");
        let result = validate_rule(&rule, dataset).expect("native rule evaluation");
        assert_eq!(result.skipped_reason, None);
        assert_eq!(result.error_count, expected_rows.len());
        assert_eq!(
            result.execution_status,
            if expected_rows.is_empty() {
                ExecutionStatus::Passed
            } else {
                ExecutionStatus::Failed
            },
        );
        assert_eq!(
            result
                .errors
                .iter()
                .filter_map(|issue| issue.row)
                .collect::<Vec<_>>(),
            expected_rows
        );
        assert_eq!(serde_json::to_value(&rule).unwrap(), before);
    }
}

#[test]
fn core_000123_through_000131_accept_y_n_and_empty() {
    check_y_n_family(&["Y", "N", ""], &[]);
}

#[test]
fn core_000123_through_000131_report_only_invalid_values() {
    check_y_n_family(
        &["Y", "N", "", "X", "y", "n", "YES", "NO"],
        &[4, 5, 6, 7, 8],
    );
}

fn check_y_n_family(values: &[&str], invalid_rows: &[usize]) {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    fs::create_dir(&rules_dir).expect("rules dir");
    let mut records = json!({
        "DOMAIN": vec!["AE"; values.len()],
        "USUBJID": (1..=values.len()).map(|i| format!("S{i}")).collect::<Vec<_>>(),
        "AESEQ": (1..=values.len()).collect::<Vec<_>>(),
    });
    for (id, variable) in Y_N_RULES {
        fs::write(
            rules_dir.join(format!("{id}.yml")),
            format!(
                r#"Core:
  Id: {id}
  Status: Published
Scope:
  Domains:
    Include: [AE]
  Classes:
    Include: [EVENTS]
Sensitivity: Record
Rule Type: Record Data
Check:
  all:
    - name: {variable}
      operator: non_empty
    - name: {variable}
      operator: is_not_contained_by
      value:
        - Y
        - N
      value_is_literal: true
Outcome:
  Message: Invalid {variable}
  Output Variables: [{variable}]
"#,
            ),
        )
        .expect("write rule");
        records[variable] = json!(values);
    }
    let dataset_path = dir.path().join("ae.json");
    fs::write(
        &dataset_path,
        serde_json::to_vec(&json!({"datasets": [{
            "filename": "ae.xpt",
            "domain": "AE",
            "records": records,
        }]}))
        .unwrap(),
    )
    .expect("write dataset");

    for oracle_compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path.clone()],
            open_rules_oracle_compat: oracle_compat,
            ..Default::default()
        })
        .expect("run validation");
        assert_eq!(outcome.results.len(), Y_N_RULES.len());
        for (id, variable) in Y_N_RULES {
            let result = outcome.results.iter().find(|r| r.rule_id == id).unwrap();
            assert_eq!(result.skipped_reason, None, "{id}");
            assert_eq!(
                result.error_count,
                invalid_rows.len(),
                "{id} compat={oracle_compat}"
            );
            assert_eq!(
                result.execution_status,
                if invalid_rows.is_empty() {
                    ExecutionStatus::Passed
                } else {
                    ExecutionStatus::Failed
                },
                "{id}"
            );
            assert_eq!(result.dataset, "AE");
            assert_eq!(
                result
                    .errors
                    .iter()
                    .filter_map(|e| e.row)
                    .collect::<Vec<_>>(),
                invalid_rows,
                "{id}"
            );
            for (issue, row) in result.errors.iter().zip(invalid_rows) {
                assert_eq!(issue.variables, vec![variable], "{id}");
                assert_eq!(issue.usubjid, Some(format!("S{row}")), "{id}");
                assert_eq!(issue.seq, Some(row.to_string()), "{id}");
            }
        }
    }
}
