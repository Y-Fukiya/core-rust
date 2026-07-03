use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::usdm_values::{json_string, value_string};

pub(crate) fn insert_quantity_columns(
    row: &mut BTreeMap<String, Value>,
    name: &str,
    quantity: &Value,
    cohorts: &[Value],
) {
    row.insert(format!("{name}.id"), json_string(quantity.get("id")));
    row.insert(
        format!("{name}(value/range)"),
        Value::String(format_quantity(quantity)),
    );
    row.insert(
        format!("{name}.present"),
        Value::Bool(quantity_present(quantity)),
    );
    row.insert(
        format!("{name}.has_unit"),
        Value::Bool(quantity_has_unit(quantity)),
    );
    let cohort_present = cohorts
        .iter()
        .map(|cohort| cohort.get(name).is_some_and(quantity_present))
        .collect::<Vec<_>>();
    let cohort_missing = cohorts
        .iter()
        .map(|cohort| {
            !cohort
                .as_object()
                .is_some_and(|object| object.contains_key(name))
        })
        .collect::<Vec<_>>();
    row.insert(
        format!("cohorts.{name}.any_present"),
        Value::Bool(cohort_present.iter().any(|present| *present)),
    );
    row.insert(
        format!("cohorts.{name}.all_present"),
        Value::Bool(!cohorts.is_empty() && cohort_present.iter().all(|present| *present)),
    );
    row.insert(
        format!("cohorts.{name}.any_missing"),
        Value::Bool(cohort_missing.iter().any(|missing| *missing)),
    );
    row.insert(
        format!("cohorts.{name}.has_unit"),
        Value::Bool(
            cohorts
                .iter()
                .any(|cohort| cohort.get(name).is_some_and(quantity_has_unit)),
        ),
    );
    row.insert(
        "cohorts.name".to_owned(),
        Value::String(format_cohort_names(cohorts)),
    );
    row.insert(
        format!("cohorts.{name}.id"),
        Value::String(format_cohort_quantity_ids(cohorts, name)),
    );
    row.insert(
        format!("cohorts.{name}(value/range)"),
        Value::String(format_cohort_quantities(cohorts, name)),
    );
}

pub(crate) fn insert_planned_sex_columns(
    row: &mut BTreeMap<String, Value>,
    planned_sex: Option<&Value>,
) {
    let values = planned_sex
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    row.insert(
        "plannedSex".to_owned(),
        Value::String(format_planned_sex(&values)),
    );
    row.insert(
        "plannedSex.invalid".to_owned(),
        Value::Bool(planned_sex_invalid(&values)),
    );
}

fn format_planned_sex(values: &[Value]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| {
                format!(
                    "{}: {} ({})",
                    value_string(value.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(value.get("decode").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(value.get("code").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn planned_sex_invalid(values: &[Value]) -> bool {
    let codes = values
        .iter()
        .filter_map(|value| value.get("code").and_then(value_string))
        .collect::<Vec<_>>();
    let distinct = codes.iter().collect::<BTreeSet<_>>().len();
    codes.len() != distinct
        || codes
            .iter()
            .any(|code| !matches!(code.as_str(), "C16576" | "C20197"))
}

fn quantity_present(quantity: &Value) -> bool {
    !quantity.is_null()
}

fn quantity_has_unit(quantity: &Value) -> bool {
    quantity.get("unit").is_some_and(|unit| !unit.is_null())
        || quantity
            .get("minValue")
            .and_then(|value| value.get("unit"))
            .is_some_and(|unit| !unit.is_null())
        || quantity
            .get("maxValue")
            .and_then(|value| value.get("unit"))
            .is_some_and(|unit| !unit.is_null())
}

fn format_quantity(quantity: &Value) -> String {
    if quantity.is_null() {
        return "null".to_owned();
    }
    if let Some(value) = quantity.get("value").and_then(value_string) {
        return if quantity.get("unit").is_some_and(|unit| !unit.is_null()) {
            format!("{value} {}", format_unit(quantity.get("unit")))
        } else {
            value
        };
    }
    if let Some(min_value) = quantity.get("minValue") {
        let max_value = quantity.get("maxValue").unwrap_or(&Value::Null);
        let min = min_value
            .get("value")
            .and_then(value_string)
            .unwrap_or_default();
        let max = max_value
            .get("value")
            .and_then(value_string)
            .unwrap_or_default();
        return if quantity_has_unit(quantity) {
            format!(
                "{min} {} to {max} {}",
                format_unit(min_value.get("unit")),
                format_unit(max_value.get("unit"))
            )
        } else {
            format!("{min} to {max}")
        };
    }
    value_string(quantity).unwrap_or_else(|| "null".to_owned())
}

fn format_unit(unit: Option<&Value>) -> String {
    let Some(unit) = unit else {
        return String::new();
    };
    let decode = unit
        .get("standardCode")
        .and_then(|code| code.get("decode"))
        .and_then(value_string)
        .unwrap_or_default();
    let code = unit
        .get("standardCode")
        .and_then(|code| code.get("code"))
        .and_then(value_string)
        .unwrap_or_default();
    if code.is_empty() {
        decode
    } else {
        format!("{decode} ({code})")
    }
}

fn format_cohort_names(cohorts: &[Value]) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(cohort.get("name").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_cohort_quantity_ids(cohorts: &[Value], name: &str) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                let quantity = cohort.get(name).unwrap_or(&Value::Null);
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    value_string(quantity.get("id").unwrap_or(&Value::Null)).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_cohort_quantities(cohorts: &[Value], name: &str) -> String {
    format!(
        "[{}]",
        cohorts
            .iter()
            .map(|cohort| {
                format!(
                    "{}: {}",
                    value_string(cohort.get("id").unwrap_or(&Value::Null)).unwrap_or_default(),
                    format_quantity(cohort.get(name).unwrap_or(&Value::Null))
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}
