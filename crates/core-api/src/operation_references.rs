use std::collections::{BTreeMap, BTreeSet};

use core_data::{dataset_column_values, dataset_setcd_values, DataError, LoadedDataset};
use core_engine::RuleValidationResult;
use core_rule_model::{
    Condition, ConditionGroup, ExecutableRule, OperationSpec, Operator, RuleType, Sensitivity,
    ValueExpr,
};
use serde_json::Value;

use crate::dataset_helpers::dataset_column_name;
use crate::json_values::json_scalar_string;
use crate::operation_execution::operation_column_values;
use crate::operation_fields::{clean_operation_identifier, operation_value};
use crate::scope_filter::{filter_datasets_by_rule_scope, scope_values};
use crate::{derive_column_from_values_with_aliases, find_dataset, operation_skipped_result};

const SETCD_NOT_IN_TX: &str = "__core_setcd_not_in_tx";

pub(crate) fn supp_reference_variable_check(rule: &ExecutableRule) -> Option<String> {
    if rule.rule_type != RuleType::RecordData
        || rule.sensitivity != Some(Sensitivity::Record)
        || rule.entities.is_some()
        || scope_values(rule.domains.as_ref(), "Include") != ["SUPP--"]
    {
        return None;
    }
    let [operation] = rule.operations.as_slice() else {
        return None;
    };
    let [child] = rule.datasets.as_deref()? else {
        return None;
    };
    // This child declaration identifies the SUPP records already in scope.
    if child.fields.len() != 3
        || child.fields.get("Name").and_then(Value::as_str) != Some("SUPP--")
        || child.fields.get("Child").and_then(Value::as_bool) != Some(true)
        || child.fields.get("Keys")?.as_array()? != &["USUBJID", "IDVAR", "IDVARVAL"]
        || operation.fields.len() != 4
        || operation.fields.get("operator")?.as_str()? != "referenced_domain_variable_names"
        || operation.fields.get("domain")?.as_str()? != "SUPP--"
        || operation.fields.get("name")?.as_str()? != "RDOMAIN"
        || rule.output_variables != ["IDVAR", "RDOMAIN"]
    {
        return None;
    }
    let output_id = operation.fields.get("id")?.as_str()?;
    if !output_id.starts_with('$') {
        return None;
    }
    let output = clean_operation_identifier(output_id);
    let ConditionGroup::All(conditions) = &rule.conditions else {
        return None;
    };
    let [ConditionGroup::Leaf(domain), ConditionGroup::Leaf(idvar), ConditionGroup::Leaf(membership)] =
        conditions.as_slice()
    else {
        return None;
    };
    (domain.target.as_deref() == Some("RDOMAIN")
        && domain.operator == Operator::Exists
        && idvar.target.as_deref() == Some("IDVAR")
        && idvar.operator == Operator::IsNotEmpty
        && membership.target.as_deref() == Some("IDVAR")
        && membership.operator == Operator::IsNotContainedBy
        && matches!(&membership.comparator, ValueExpr::ColumnRef(name) if name == &output)
        && [domain, idvar, membership]
            .iter()
            .all(|check| check.options.extra.is_empty())
        && !output.is_empty())
    .then_some(output)
}

pub(crate) fn bind_supp_reference_variable_check(
    rule: &mut ExecutableRule,
    datasets: &[LoadedDataset],
    output: String,
) -> Result<Vec<LoadedDataset>, RuleValidationResult> {
    let scoped = filter_datasets_by_rule_scope(rule, datasets);
    if scoped.is_empty() {
        return Err(operation_skipped_result(
            rule,
            "no supplemental dataset is in rule scope",
        ));
    }
    let mut variables_by_domain = BTreeMap::<String, BTreeSet<String>>::new();
    for dataset in datasets {
        if let Some(domain) = dataset.metadata.domain.as_deref() {
            variables_by_domain
                .entry(domain.trim().to_ascii_uppercase())
                .or_default()
                .extend(
                    dataset
                        .frame()
                        .get_column_names()
                        .iter()
                        .map(|name| name.as_str().to_ascii_uppercase()),
                );
        }
    }
    let derived = scoped
        .iter()
        .map(|dataset| {
            let required = |name| {
                dataset_column_name(dataset, name).ok_or_else(|| {
                    operation_skipped_result(
                        rule,
                        format!("dataset {} has no {name} column", dataset.metadata.name),
                    )
                })
            };
            let columns = |name| {
                dataset_column_values(dataset, &required(name)?)
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))
            };
            let domains = columns("RDOMAIN")?;
            let idvars = columns("IDVAR")?;
            if dataset_column_name(dataset, &output).is_some() {
                return Err(operation_skipped_result(
                    rule,
                    format!("output column {output} already exists"),
                ));
            }
            let invalid = domains
                .iter()
                .zip(idvars.iter())
                .map(|(domain, idvar)| {
                    let domain = domain
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_uppercase();
                    Value::Bool(!idvar.as_str().is_some_and(|name| {
                        variables_by_domain
                            .get(&domain)
                            .is_some_and(|columns| columns.contains(name))
                    }))
                })
                .collect::<Vec<_>>();
            derive_column_from_values_with_aliases(dataset, &output, &invalid)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if let ConditionGroup::All(conditions) = &mut rule.conditions {
        conditions[2] = ConditionGroup::Leaf(Condition {
            target: Some(output),
            operator: Operator::EqualTo,
            comparator: ValueExpr::Literal(Value::Bool(true)),
            options: Default::default(),
        });
    }
    Ok(derived)
}

pub(crate) fn is_tx_setcd_reference_rule(rule: &ExecutableRule) -> bool {
    if rule.core_id != crate::engine_semantics::CORE_000609
        || rule.rule_type != RuleType::RecordData
        || rule.sensitivity != Some(Sensitivity::Record)
        || rule.entities.is_some()
        || rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
    {
        return false;
    }
    let [operation] = rule.operations.as_slice() else {
        return false;
    };
    let ConditionGroup::All(conditions) = &rule.conditions else {
        return false;
    };
    let [ConditionGroup::Leaf(condition)] = conditions.as_slice() else {
        return false;
    };
    operation.fields.len() == 4
        && [
            ("domain", "TX"),
            ("id", "$tx_setcd"),
            ("name", "SETCD"),
            ("operator", "distinct"),
        ]
        .into_iter()
        .all(|(key, expected)| operation.fields.get(key).and_then(Value::as_str) == Some(expected))
        && condition.target.as_deref() == Some("SETCD")
        && condition.operator == Operator::IsNotContainedBy
        && matches!(&condition.comparator, ValueExpr::ColumnRef(column) if column == "tx_setcd")
        && condition.options.extra.is_empty()
}

pub(crate) fn bind_tx_setcd_reference(
    rule: &mut ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let tx = find_dataset(datasets, "TX")
        .ok_or_else(|| operation_skipped_result(rule, "dataset TX was not loaded"))?;
    let source_column = dataset_column_name(tx, "SETCD")
        .ok_or_else(|| operation_skipped_result(rule, "dataset TX has no SETCD column"))?;
    let source = dataset_setcd_values(tx, &source_column)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let setcds: BTreeSet<&str> = source
        .iter()
        .filter_map(Value::as_str)
        .filter(|value| !value.is_empty())
        .collect();
    let scoped = filter_datasets_by_rule_scope(rule, datasets)
        .into_iter()
        .filter_map(|dataset| dataset_column_name(&dataset, "SETCD").map(|name| (dataset, name)))
        .map(|(dataset, column)| {
            let not_in_tx = dataset_setcd_values(&dataset, &column)?
                .iter()
                .map(|value| {
                    Value::Bool(!value.as_str().is_some_and(|value| setcds.contains(value)))
                })
                .collect::<Vec<_>>();
            derive_column_from_values_with_aliases(&dataset, SETCD_NOT_IN_TX, &not_in_tx)
        })
        .collect::<std::result::Result<Vec<_>, DataError>>()
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;

    rule.conditions = ConditionGroup::Leaf(Condition {
        target: Some(SETCD_NOT_IN_TX.to_owned()),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::Bool(true)),
        options: Default::default(),
    });
    if rule.output_variables.is_empty() {
        rule.output_variables = vec!["SETCD".to_owned(), "$tx_setcd".to_owned()];
    }
    Ok(scoped)
}

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
