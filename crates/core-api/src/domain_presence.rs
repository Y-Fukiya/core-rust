use core_data::{derive_literal_column, filter_dataset_by_mask, LoadedDataset};
use core_engine::{RuleValidationResult, SkippedReason};
use core_rule_model::ExecutableRule;
use serde_json::Value;

use crate::operation_fields::{operation_name, string_field};
use crate::{dataset_has_column, find_dataset, operation_skipped_result, presence_target_columns};

pub(crate) fn domain_presence_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let targets = presence_target_columns(&rule.conditions);
    if targets.is_empty() {
        return Ok(Vec::new());
    }

    let anchor = targets
        .iter()
        .find_map(|target| find_dataset(datasets, target))
        .or_else(|| datasets.first())
        .ok_or_else(|| {
            RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} has no datasets for domain presence check",
                    rule.core_id
                ),
            )
        })?;

    let mut dataset = filter_dataset_by_mask(
        anchor,
        &(0..anchor.summary().row_count)
            .map(|row| row == 0)
            .collect::<Vec<_>>(),
    )
    .map_err(|source| operation_skipped_result(rule, source.to_string()))?;

    let primary = targets
        .iter()
        .find_map(|target| find_dataset(datasets, target).map(|dataset| (target, dataset)));
    if let Some((target, source_dataset)) = primary {
        dataset.metadata.name = target.to_ascii_uppercase();
        dataset.metadata.domain = Some(target.to_ascii_uppercase());
        dataset.metadata.filename = source_dataset.metadata.filename.clone();
        dataset.metadata.full_path = source_dataset.metadata.full_path.clone();
    }

    for target in targets {
        if let Some(source_dataset) = find_dataset(datasets, &target) {
            let value = Value::String(source_dataset.metadata.filename.clone());
            if !dataset_has_column(&dataset, &target) {
                dataset = derive_literal_column(&dataset, &target, &value)
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
            }
        }
    }

    for operation in &rule.operations {
        if operation_name(operation).as_deref() == Some("variable_exists") {
            let output = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$variable_exists".to_owned());
            let Some(source_column) = string_field(
                operation,
                &[
                    "name",
                    "source_column",
                    "value_column",
                    "measure",
                    "variable",
                ],
            ) else {
                return Err(operation_skipped_result(
                    rule,
                    "variable_exists operation is missing a source variable",
                ));
            };
            let exists = datasets
                .iter()
                .any(|source_dataset| dataset_has_column(source_dataset, &source_column));
            dataset = derive_literal_column(&dataset, &output, &Value::Bool(exists))
                .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
        }
    }

    Ok(vec![dataset])
}
