use core_data::{DataError, LoadedDataset};
use core_rule_model::OperationSpec;
use serde_json::Value;

use crate::derive_column_from_values_with_aliases;
use crate::json_values::json_scalar_string;
use crate::operation_execution::operation_column_values;
use crate::operation_fields::operation_value;

pub(crate) fn operation_reference_values(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    key: &str,
) -> std::result::Result<Vec<String>, DataError> {
    let Some(value) = operation_value(operation, &[key]) else {
        return Ok(vec![String::new(); dataset.summary().row_count]);
    };
    if let Some(reference) = value.as_str() {
        let values = operation_column_values(dataset, reference)
            .unwrap_or_else(|_| vec![Value::String(String::new()); dataset.summary().row_count]);
        return Ok(values
            .iter()
            .map(|value| json_scalar_string(value).unwrap_or_default())
            .collect());
    }
    let literal = json_scalar_string(value).unwrap_or_default();
    Ok(vec![literal; dataset.summary().row_count])
}

pub(crate) fn optional_operation_reference_values(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    key: &str,
) -> std::result::Result<Option<Vec<String>>, DataError> {
    if operation_value(operation, &[key]).is_none() {
        return Ok(None);
    }
    operation_reference_values(dataset, operation, key).map(Some)
}

pub(crate) fn derive_dataset_filtered_variables_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
    key_name: &str,
    key_value: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "get_dataset_filtered_variables operation requires an output column".to_owned(),
        ));
    }
    let variables = dataset_filtered_variable_names(dataset, key_name, key_value);
    let value = Value::String(format_column_list_literal(&variables));
    let values = (0..dataset.summary().row_count)
        .map(|_| value.clone())
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn dataset_filtered_variable_names(
    dataset: &LoadedDataset,
    key_name: &str,
    key_value: &str,
) -> Vec<String> {
    dataset
        .metadata
        .variables
        .iter()
        .filter(|variable| {
            variable_matches_filter(dataset, variable.name.as_str(), key_name, key_value)
        })
        .map(|variable| variable.name.clone())
        .collect()
}

fn variable_matches_filter(
    dataset: &LoadedDataset,
    variable_name: &str,
    key_name: &str,
    key_value: &str,
) -> bool {
    if key_name.eq_ignore_ascii_case("role") && key_value.eq_ignore_ascii_case("Timing") {
        return is_timing_variable(dataset, variable_name);
    }
    false
}

fn is_timing_variable(dataset: &LoadedDataset, variable_name: &str) -> bool {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let name = variable_name.to_ascii_uppercase();
    matches!(name.as_str(), "VISITNUM" | "VISIT" | "EPOCH")
        || name == format!("{domain}DTC")
        || name == format!("{domain}DY")
        || name == format!("{domain}TPT")
        || name == format!("{domain}TPTNUM")
        || name == format!("{domain}ELTM")
        || name == format!("{domain}TPTREF")
        || name == format!("{domain}RFTDTC")
}

fn format_column_list_literal(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
