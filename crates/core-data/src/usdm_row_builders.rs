use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) fn usdm_duration_row(duration: &Value, path: &str) -> BTreeMap<String, Value> {
    let text = duration.get("text").and_then(value_string);
    let quantity = duration.get("quantity");
    let quantity_present = quantity.is_some_and(|value| !value.is_null());
    let text_present = text.as_ref().is_some_and(|value| !value.is_empty());
    let duration_will_vary = duration.get("durationWillVary").and_then(Value::as_bool);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(duration.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(duration.get("id")));
    row.insert(
        "text".to_owned(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "quantity".to_owned(),
        quantity
            .map(format_quantity_value)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "quantity(value/range)".to_owned(),
        quantity
            .map(format_quantity_value)
            .map(Value::String)
            .unwrap_or_else(|| Value::String("Missing".to_owned())),
    );
    row.insert(
        "durationWillVary".to_owned(),
        duration_will_vary.map(Value::Bool).unwrap_or(Value::Null),
    );
    row.insert(
        "duration_missing_text_and_quantity".to_owned(),
        Value::Bool(!text_present && !quantity_present),
    );
    row.insert(
        "duration_vary_quantity_conflict".to_owned(),
        Value::Bool(match duration_will_vary {
            Some(true) => quantity_present,
            Some(false) => !quantity_present,
            None => false,
        }),
    );
    row
}

pub(crate) fn usdm_range_row(range: &Value, path: &str) -> BTreeMap<String, Value> {
    let min = range.get("minValue");
    let max = range.get("maxValue");
    let min_value = min
        .and_then(|value| value.get("value"))
        .and_then(Value::as_f64);
    let max_value = max
        .and_then(|value| value.get("value"))
        .and_then(Value::as_f64);
    let min_unit = quantity_unit_code(min);
    let max_unit = quantity_unit_code(max);
    let same_or_missing_units = min_unit == max_unit;
    let unit_xor = min_unit.is_some() ^ max_unit.is_some();
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(range.get("instanceType")),
    );
    row.insert(
        "minValue".to_owned(),
        min.map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "maxValue".to_owned(),
        max.map(format_quantity_single_with_missing_unit)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    row.insert(
        "range_min_not_less_than_max".to_owned(),
        Value::Bool(
            same_or_missing_units
                && min_value
                    .zip(max_value)
                    .is_some_and(|(min_value, max_value)| min_value >= max_value),
        ),
    );
    row.insert("range_unit_xor".to_owned(), Value::Bool(unit_xor));
    row
}

pub(crate) fn usdm_person_name_row(person_name: &Value, path: &str) -> BTreeMap<String, Value> {
    let family_name = person_name.get("familyName").and_then(value_string);
    let text = person_name.get("text").and_then(value_string);
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "instanceType".to_owned(),
        json_string(person_name.get("instanceType")),
    );
    row.insert(
        "familyName".to_owned(),
        family_name.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "text".to_owned(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    row.insert(
        "person_name_missing_text_and_family_name".to_owned(),
        Value::Bool(
            person_name
                .get("familyName")
                .and_then(value_string)
                .is_none_or(|value| value.is_empty())
                && person_name
                    .get("text")
                    .and_then(value_string)
                    .is_none_or(|value| value.is_empty()),
        ),
    );
    row
}

pub(crate) fn usdm_address_row(
    address: &Value,
    path: &str,
    organization: &Value,
) -> BTreeMap<String, Value> {
    let fields = [
        "text",
        "lines",
        "city",
        "district",
        "state",
        "postalCode",
        "country",
    ];
    let all_missing = fields
        .iter()
        .all(|field| address.get(*field).is_none_or(usdm_address_field_is_blank));
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert(
        "Organization.id".to_owned(),
        json_string(organization.get("id")),
    );
    row.insert(
        "Organization.name".to_owned(),
        json_string(organization.get("name")),
    );
    for field in fields {
        row.insert(field.to_owned(), jsonata_exists_rep(address.get(field)));
    }
    row.insert("address_all_blank".to_owned(), Value::Bool(all_missing));
    row
}

fn usdm_address_field_is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.is_empty(),
        Value::Object(_) => false,
        _ => value_string(value).is_none_or(|value| value.is_empty()),
    }
}

fn jsonata_exists_rep(value: Option<&Value>) -> Value {
    match value {
        None => Value::String("Missing".to_owned()),
        Some(Value::Null) => Value::Null,
        Some(Value::Array(values)) => {
            let values: Vec<String> = values.iter().filter_map(value_string).collect();
            Value::String(format_string_list(&values))
        }
        Some(value) => value_string(value)
            .map(Value::String)
            .unwrap_or_else(|| Value::String(value.to_string())),
    }
}

fn json_string(value: Option<&Value>) -> Value {
    value
        .and_then(value_string)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn format_string_list(values: &[String]) -> String {
    format!("[{}]", values.join(", "))
}

fn format_code(code: Option<&Value>) -> String {
    let Some(code) = code else {
        return String::new();
    };
    let decode = code
        .get("decode")
        .and_then(value_string)
        .unwrap_or_default();
    let code_value = code.get("code").and_then(value_string).unwrap_or_default();
    if code_value.is_empty() {
        decode
    } else {
        format!("{decode} ({code_value})")
    }
}

fn format_quantity_value(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            if object.contains_key("value") {
                return format_quantity_single(value);
            }
            if object.contains_key("minValue") || object.contains_key("maxValue") {
                let min = object
                    .get("minValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                let max = object
                    .get("maxValue")
                    .map(format_quantity_single)
                    .unwrap_or_default();
                return format!("{min} to {max}");
            }
            value.to_string()
        }
        Value::Null => "Missing".to_owned(),
        _ => value_string(value).unwrap_or_else(|| value.to_string()),
    }
}

fn format_quantity_single(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_default();
    if unit.is_empty() {
        quantity
    } else {
        format!("{quantity} {unit}")
    }
}

fn format_quantity_single_with_missing_unit(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value_string(value).unwrap_or_else(|| value.to_string());
    };
    let quantity = object
        .get("value")
        .and_then(value_string)
        .unwrap_or_default();
    let unit = object
        .get("unit")
        .and_then(|unit| unit.get("standardCode"))
        .map(|code| format_code(Some(code)))
        .unwrap_or_else(|| "unit not specified".to_owned());
    format!("{quantity} {unit}")
}

fn quantity_unit_code(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value.get("unit"))
        .and_then(|unit| unit.get("standardCode"))
        .and_then(|code| code.get("code"))
        .and_then(value_string)
}
