use super::*;
use serde_json::json;

const MEMBERSHIP_OPERATORS: [&str; 4] = [
    "is_contained_by",
    "is_not_contained_by",
    "is_contained_by_case_insensitive",
    "is_not_contained_by_case_insensitive",
];

fn comparator(operator: &str, value: Value, literal: bool) -> ValueExpr {
    let condition = normalize_condition_value(&json!({
        "name": "AESCAN",
        "operator": operator,
        "value": value,
        "value_is_literal": literal,
    }))
    .expect("normalize condition");
    let ConditionGroup::Leaf(condition) = condition else {
        panic!("expected leaf condition");
    };
    condition.comparator
}

#[test]
fn explicit_literal_membership_arrays_preserve_literal_and_round_trip() {
    for operator in MEMBERSHIP_OPERATORS {
        for values in [
            json!(["Y", "N"]),
            json!([]),
            json!(["$OTHER", "OTHER", "DM.M"]),
        ] {
            let parsed = comparator(operator, values.clone(), true);
            assert_eq!(
                parsed,
                ValueExpr::Literal(values.clone()),
                "{operator} {values}",
            );
            let serialized = serde_json::to_value(&parsed).expect("serialize comparator");
            assert_eq!(serialized, json!({"literal": values}));
            assert_eq!(
                serde_json::from_value::<ValueExpr>(serialized).expect("deserialize comparator"),
                parsed,
            );
        }
    }
}

#[test]
fn yaml_explicit_literal_membership_preserves_bare_y_n() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rule.yml");
    std::fs::write(
        &path,
        r#"Core:
  Id: CORE-000123
Scope: {}
Rule Type: Record Data
Check:
  name: AESCAN
  operator: is_not_contained_by
  value:
    - Y
    - N
  value_is_literal: true
Outcome:
  Message: Invalid AESCAN
"#,
    )
    .expect("write rule");
    let rule = load_rule_file(path).expect("load YAML rule");
    let ConditionGroup::Leaf(condition) = rule.conditions else {
        panic!("expected leaf condition");
    };
    assert_eq!(condition.comparator, ValueExpr::Literal(json!(["Y", "N"])));
}

#[test]
fn explicit_literal_scalars_and_objects_are_not_reinterpreted() {
    for operator in MEMBERSHIP_OPERATORS.into_iter().chain(["equal_to"]) {
        for value in [
            json!("$OTHER"),
            json!("OTHER"),
            json!(null),
            json!(true),
            json!(7),
            json!({"column_ref": "OTHER"}),
            json!({"column": "OTHER"}),
            json!({"list": ["Y", "N"]}),
            json!({"literal": ["Y", "N"]}),
        ] {
            assert_eq!(
                comparator(operator, value.clone(), true),
                ValueExpr::Literal(value),
                "{operator}",
            );
        }
        assert_eq!(
            comparator(operator, json!("$OTHER"), false),
            ValueExpr::ColumnRef("OTHER".to_owned()),
        );
    }
}

#[test]
fn non_membership_literal_arrays_keep_their_existing_shape() {
    for operator in [
        "contains_all",
        "not_contains_all",
        "shares_no_elements_with",
        "is_not_ordered_subset_of",
        "is_unique_set",
        "is_not_unique_set",
        "equal_to",
        "prefix_is_not_contained_by",
        "suffix_is_not_contained_by",
    ] {
        let values = json!(["OTHER", "$OTHER"]);
        assert_eq!(
            comparator(operator, values.clone(), true),
            ValueExpr::Literal(values.clone()),
            "{operator}",
        );
        assert_eq!(
            comparator(operator, values.clone(), false),
            ValueExpr::List(values.as_array().unwrap().clone()),
            "{operator}",
        );
    }
}
