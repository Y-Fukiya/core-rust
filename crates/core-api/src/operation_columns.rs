use std::collections::BTreeSet;

use core_data::{
    dataset_column_values, derive_column_from_column, derive_column_from_values,
    derive_literal_column, DataError, LoadedDataset,
};
use core_engine::RuleValidationResult;
use core_rule_model::{ExecutableRule, OperationSpec};
use serde_json::Value;

use crate::metadata_support::operation_dataset_name;
use crate::operation_fields::{
    clean_operation_identifier, is_quoted_literal, operation_function_argument,
    operation_function_arguments, operation_string_literal,
};
use crate::{find_dataset, operation_skipped_result};

pub(crate) fn derive_column_from_values_with_aliases(
    dataset: &LoadedDataset,
    column_name: &str,
    values: &[Value],
) -> std::result::Result<LoadedDataset, DataError> {
    let derived = derive_column_from_values(dataset, column_name, values)?;
    let clean_column_name = clean_operation_identifier(column_name);
    if clean_column_name == column_name {
        Ok(derived)
    } else {
        derive_column_from_values(&derived, &clean_column_name, values)
    }
}

pub(crate) fn reference_dataset_variable_names(dataset: &LoadedDataset) -> Vec<String> {
    let names = if dataset.metadata.variables.is_empty() {
        dataset.summary().columns
    } else {
        dataset
            .metadata
            .variables
            .iter()
            .map(|variable| variable.name.clone())
            .collect()
    };
    names
        .into_iter()
        .filter(|name| !name.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn dataset_has_variable(dataset: &LoadedDataset, column: &str) -> bool {
    reference_dataset_variable_names(dataset)
        .iter()
        .any(|name| name.eq_ignore_ascii_case(column))
}

pub(crate) fn expand_dataset_domain_placeholder(dataset: &LoadedDataset, name: &str) -> String {
    let Some(suffix) = name.strip_prefix("--") else {
        return name.to_owned();
    };
    let Some(domain) = dataset
        .metadata
        .domain
        .as_deref()
        .filter(|domain| !domain.trim().is_empty())
        .or_else(|| {
            (!dataset.metadata.name.trim().is_empty()).then_some(dataset.metadata.name.as_str())
        })
    else {
        return name.to_owned();
    };
    format!(
        "{}{}",
        domain.trim().to_ascii_uppercase(),
        suffix.to_ascii_uppercase()
    )
}

pub(crate) fn operation_input_datasets(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if let Some(name) = operation_dataset_name(operation) {
        let Some(dataset) = find_dataset(datasets, &name) else {
            return Err(operation_skipped_result(
                rule,
                format!("dataset {name} was not available for operation"),
            ));
        };
        Ok(vec![dataset.clone()])
    } else {
        Ok(datasets.to_vec())
    }
}

pub(crate) fn derive_jsonata_column(
    dataset: &LoadedDataset,
    column: &str,
    expression: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let expression = expression.trim();
    if let Some(argument) = operation_function_argument(expression, &["$uppercase", "uppercase"]) {
        return derive_transformed_column(dataset, column, argument, |value| {
            value.to_ascii_uppercase()
        });
    }
    if let Some(argument) = operation_function_argument(expression, &["$lowercase", "lowercase"]) {
        return derive_transformed_column(dataset, column, argument, |value| {
            value.to_ascii_lowercase()
        });
    }
    if let Some(argument) = operation_function_argument(expression, &["$trim", "trim"]) {
        return derive_transformed_column(dataset, column, argument, |value| {
            value.trim().to_owned()
        });
    }
    if let Some(args) = operation_function_arguments(expression, &["$concat", "concat"]) {
        let mut columns = Vec::new();
        for arg in &args {
            if !is_quoted_literal(arg) {
                columns.push((
                    arg,
                    dataset_column_values(dataset, &clean_operation_identifier(arg))?,
                ));
            }
        }
        let values = (0..dataset.frame().height())
            .map(|row| {
                let mut value = String::new();
                for arg in &args {
                    if let Some(literal) = operation_string_literal(arg) {
                        value.push_str(&literal);
                    } else if let Some((_name, values)) = columns.iter().find(|(name, _values)| {
                        clean_operation_identifier(name) == clean_operation_identifier(arg)
                    }) {
                        value.push_str(values.get(row).and_then(Value::as_str).unwrap_or_default());
                    }
                }
                Value::String(value)
            })
            .collect::<Vec<_>>();
        return derive_column_from_values(dataset, column, &values);
    }
    if let Some(literal) = operation_string_literal(expression) {
        return derive_literal_column(dataset, column, &Value::String(literal));
    }
    derive_column_from_column(dataset, column, &clean_operation_identifier(expression))
}

fn derive_transformed_column(
    dataset: &LoadedDataset,
    column: &str,
    source_column: &str,
    transform: impl Fn(&str) -> String,
) -> std::result::Result<LoadedDataset, DataError> {
    let values = dataset_column_values(dataset, &clean_operation_identifier(source_column))?
        .into_iter()
        .map(|value| match value {
            Value::String(value) => Value::String(transform(&value)),
            Value::Null => Value::Null,
            other => Value::String(transform(&other.to_string())),
        })
        .collect::<Vec<_>>();
    derive_column_from_values(dataset, column, &values)
}
