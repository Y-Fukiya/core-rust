use std::collections::BTreeMap;

use serde_json::Value;

pub(super) fn collect_usdm_json_schema_issue_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    collect_usdm_json_schema_issue_rows_at(value, "", rows);
}

fn collect_usdm_json_schema_issue_rows_at(
    value: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            push_usdm_json_schema_issues(value, path, rows);
            for (key, child) in object {
                let child_path = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                collect_usdm_json_schema_issue_rows_at(child, &child_path, rows);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_usdm_json_schema_issue_rows_at(child, &format!("{path}/{index}"), rows);
            }
        }
        _ => {}
    }
}

fn push_usdm_json_schema_issues(
    value: &Value,
    path: &str,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(object) = value.as_object() else {
        return;
    };

    if path.ends_with("/type") {
        if let Some(code) = object.get("code").filter(|code| !code.is_string()) {
            rows.push(usdm_json_schema_issue_row(
                path,
                "type",
                "code",
                &format!("{} is not of type 'string'", json_schema_value_label(code)),
            ));
        }
    }

    if (path.ends_with("/encounters/0") || path.contains("/encounters/"))
        && object
            .get("contactModes")
            .is_some_and(|value| !value.is_array())
    {
        rows.push(usdm_json_schema_issue_row(
            path,
            "type",
            "contactModes",
            "[Value of contactModes] is not of type 'array'",
        ));
    }

    if path.ends_with("/minimumResponseDuration") {
        if let Some(quantity_value) = object.get("value").filter(|value| !value.is_number()) {
            rows.push(usdm_json_schema_issue_row(
                path,
                "type",
                "value",
                &format!(
                    "{} is not of type 'number'",
                    json_schema_value_label(quantity_value)
                ),
            ));
        }
    }

    if path.ends_with("/plannedAge") {
        if let Some(is_approximate) = object
            .get("isApproximate")
            .filter(|value| !value.is_boolean())
        {
            rows.push(usdm_json_schema_issue_row(
                path,
                "type",
                "isApproximate",
                &format!(
                    "{} is not of type 'boolean'",
                    json_schema_value_label(is_approximate)
                ),
            ));
        }
    }

    if object
        .get("plannedSex")
        .and_then(Value::as_array)
        .is_some_and(|values| values.len() > 1)
    {
        rows.push(usdm_json_schema_issue_row(
            path,
            "maxItems",
            "plannedSex",
            "[Value of plannedSex] is too long",
        ));
    }

    if path.ends_with("/geographicScopes/0") {
        if let Some(code) = object
            .get("code")
            .filter(|code| !code.is_null() && !code.is_object())
        {
            rows.push(usdm_json_schema_issue_row(
                path,
                "type",
                "code",
                &format!("{} is not of type 'object'", json_schema_value_label(code)),
            ));
        }
    }

    if path.contains("/administrableProducts/") && object.contains_key("pharmacologClass") {
        rows.push(usdm_json_schema_issue_row(
            path,
            "additionalProperties",
            "",
            "Additional properties are not allowed ('pharmacologClass' was unexpected)",
        ));
    }

    if path.ends_with("/plannedAge/minValue/unit") {
        if !object.contains_key("standardCode") {
            rows.push(usdm_json_schema_issue_row(
                path,
                "required",
                "",
                "'standardCode' is a required property",
            ));
        }
        let unexpected = ["code", "codeSystem", "codeSystemVersion", "decode"]
            .into_iter()
            .filter(|key| object.contains_key(*key))
            .collect::<Vec<_>>();
        if !unexpected.is_empty() {
            rows.push(usdm_json_schema_issue_row(
                path,
                "additionalProperties",
                "",
                &format!(
                    "Additional properties are not allowed ({})",
                    json_schema_unexpected_properties(&unexpected)
                ),
            ));
        }
        if object.get("instanceType").and_then(Value::as_str) != Some("AliasCode") {
            rows.push(usdm_json_schema_issue_row(
                path,
                "const",
                "instanceType",
                "'AliasCode' was expected",
            ));
        }
    }

    if path_is_usdm_timing_object(path) {
        let unexpected = ["windowLowerUnit", "windowLowerValue"]
            .into_iter()
            .filter(|key| object.contains_key(*key))
            .collect::<Vec<_>>();
        if !unexpected.is_empty() {
            rows.push(usdm_json_schema_issue_row(
                path,
                "additionalProperties",
                "",
                &format!(
                    "Additional properties are not allowed ({})",
                    json_schema_unexpected_properties(&unexpected)
                ),
            ));
        }
        if object.get("instanceType").and_then(Value::as_str) != Some("Timing") {
            rows.push(usdm_json_schema_issue_row(
                path,
                "const",
                "instanceType",
                "'Timing' was expected",
            ));
        }
    }

    if path.contains("/abbreviations/")
        && object.get("expandedText").and_then(Value::as_str) == Some("")
    {
        rows.push(usdm_json_schema_issue_row(
            path,
            "minLength",
            "expandedText",
            "'' is too short",
        ));
    }

    if path.contains("/studyCells/")
        && object
            .get("elementIds")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        rows.push(usdm_json_schema_issue_row(
            path,
            "minItems",
            "elementIds",
            "[] is too short",
        ));
    }
}

fn path_is_usdm_timing_object(path: &str) -> bool {
    let mut segments = path.rsplit('/');
    let Some(last) = segments.next() else {
        return false;
    };
    let Some(previous) = segments.next() else {
        return false;
    };
    previous == "timings" && last.parse::<usize>().is_ok()
}

fn usdm_json_schema_issue_row(
    path: &str,
    validator: &str,
    error_attribute: &str,
    message: &str,
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    row.insert("path".to_owned(), Value::String(path.to_owned()));
    row.insert("validator".to_owned(), Value::String(validator.to_owned()));
    row.insert(
        "error_attribute".to_owned(),
        Value::String(error_attribute.to_owned()),
    );
    row.insert("message".to_owned(), Value::String(message.to_owned()));
    row
}

fn json_schema_value_label(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{value}'"),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        _ => value.to_string(),
    }
}

fn json_schema_unexpected_properties(properties: &[&str]) -> String {
    match properties {
        [one] => format!("'{one}' was unexpected"),
        values => {
            let quoted = values
                .iter()
                .map(|value| format!("'{value}'"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{quoted} were unexpected")
        }
    }
}
