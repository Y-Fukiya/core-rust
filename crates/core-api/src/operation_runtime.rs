use std::collections::{BTreeMap, BTreeSet};

use core_data::{
    anti_join_dataset_on, dataset_column_values, deduplicate_dataset_by_columns,
    derive_column_from_column, derive_literal_column, drop_dataset_columns, filter_dataset_by_mask,
    group_stat_dataset, inner_join_dataset_on, left_join_dataset_on, rename_dataset_columns,
    row_number_dataset, select_dataset_columns, semi_join_dataset_on, sort_dataset_by_columns,
    DataError, LoadedDataset,
};
use core_engine::{evaluate_condition_group, RuleValidationResult};
use core_rule_model::{normalize_condition_value, ExecutableRule, OperationSpec};
use serde_json::Value;

use crate::dataset_helpers::dataset_has_column;
use crate::json_values::{json_distinct_value_string, json_scalar_string};
use crate::metadata_support::{has_group_aliases, operation_dataset_name};
use crate::open_rules_compat::has_oracle_gap_rule_id;
use crate::operation_columns::{
    derive_column_from_values_with_aliases, derive_jsonata_column, operation_input_datasets,
    reference_dataset_variable_names,
};
use crate::operation_datasets::{
    derive_codelist_extensible_dataset, derive_codelist_terms_dataset, derive_domain_label_dataset,
    derive_mapped_dataset, derive_metadata_dataset, derive_parent_model_column_order_dataset,
    derive_split_by_dataset, derive_study_day_dataset, derive_study_domains_dataset,
    derive_valid_codelist_dates_dataset, derive_variable_count_dataset,
    derive_xhtml_errors_dataset,
};
use crate::operation_execution::{
    apply_operation_inline_filter, filtered_group_count_key,
    group_count_dataset_with_inline_filter, group_distinct_values_dataset_with_aliases,
    operation_column_values, operation_group_key_columns, operation_inline_filter_mask,
};
use crate::operation_fields::{
    bool_field, is_join_operation, normalize_operation_key, operation_name, operation_value,
    rename_pair, string_field, string_list_field, string_map_field,
};
use crate::operation_references::derive_dataset_filtered_variables_dataset;
use crate::{
    condition_targets_column, dataset_matches_name, engine_semantics, find_dataset,
    is_supported_reference_distinct_rule, join_keys, join_skipped_result, operation_skipped_result,
};

pub(crate) fn execute_join_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    current_datasets: &[LoadedDataset],
    original_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some((left_keys, right_keys)) = join_keys(operation) else {
        return Err(join_skipped_result(rule, "join operation is missing keys"));
    };
    let Some(left_name) = string_field(
        operation,
        &[
            "left",
            "left_dataset",
            "primary",
            "primary_dataset",
            "dataset",
        ],
    ) else {
        return Err(join_skipped_result(
            rule,
            "join operation is missing left dataset",
        ));
    };
    let Some(right_name) = string_field(
        operation,
        &[
            "right",
            "right_dataset",
            "with",
            "secondary",
            "secondary_dataset",
        ],
    ) else {
        return Err(join_skipped_result(
            rule,
            "join operation is missing right dataset",
        ));
    };

    let Some(left) = find_dataset(current_datasets, &left_name) else {
        return Err(join_skipped_result(
            rule,
            format!("left dataset {left_name} was not loaded"),
        ));
    };
    let Some(right) = find_dataset(current_datasets, &right_name)
        .or_else(|| find_dataset(original_datasets, &right_name))
    else {
        return Err(join_skipped_result(
            rule,
            format!("right dataset {right_name} was not loaded"),
        ));
    };

    let prefix =
        string_field(operation, &["prefix"]).unwrap_or_else(|| format!("{}.", right.metadata.name));
    let name = operation_name(operation).unwrap_or_default();
    let joined = match name.as_str() {
        "inner_join" => inner_join_dataset_on(left, right, &left_keys, &right_keys, &prefix),
        "semi_join" => semi_join_dataset_on(left, right, &left_keys, &right_keys),
        "anti_join" => anti_join_dataset_on(left, right, &left_keys, &right_keys),
        _ => left_join_dataset_on(left, right, &left_keys, &right_keys, &prefix),
    };
    joined
        .map(|dataset| vec![dataset])
        .map_err(|source| join_skipped_result(rule, source.to_string()))
}

pub(crate) fn initial_operation_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some(operation) = rule
        .operations
        .iter()
        .find(|operation| !is_join_operation(operation))
    else {
        return Ok(datasets.to_vec());
    };

    if is_scope_external_reference_distinct_operation(rule, operation, datasets) {
        return Ok(datasets.to_vec());
    }

    if is_external_group_date_operation(operation, datasets) {
        return Ok(datasets.to_vec());
    }
    if is_external_group_min_max_operation(operation, datasets) {
        return Ok(datasets.to_vec());
    }

    if is_scope_wide_reference_distinct_operation(rule, operation) {
        if let Some(name) = operation_dataset_name(operation) {
            let scoped = datasets
                .iter()
                .filter(|dataset| !dataset_matches_name(dataset, &name))
                .cloned()
                .collect::<Vec<_>>();
            return Ok(scoped);
        }
    }

    if let Some(name) = operation_dataset_name(operation) {
        if name.contains("--") {
            let matching = datasets
                .iter()
                .filter(|dataset| dataset_matches_name(dataset, &name))
                .cloned()
                .collect::<Vec<_>>();
            if !matching.is_empty() {
                return Ok(matching);
            }
        }
        if has_group_aliases(operation) && find_dataset(datasets, &name).is_none() {
            return Ok(datasets.to_vec());
        }
        if should_preserve_scoped_datasets_for_targeted_operation(rule, datasets, &name) {
            return Ok(datasets.to_vec());
        }
        let Some(dataset) = find_dataset(datasets, &name) else {
            return Err(operation_skipped_result(
                rule,
                format!("dataset {name} was not loaded"),
            ));
        };
        Ok(vec![dataset.clone()])
    } else {
        Ok(datasets.to_vec())
    }
}

fn should_preserve_scoped_datasets_for_targeted_operation(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    target_dataset: &str,
) -> bool {
    datasets.len() > 1
        && datasets
            .iter()
            .any(|dataset| !dataset_matches_name(dataset, target_dataset))
        && condition_targets_column(&rule.conditions, "DOMAIN")
}

fn is_scope_external_reference_distinct_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    datasets: &[LoadedDataset],
) -> bool {
    if !is_supported_reference_distinct_rule(rule) {
        return false;
    }

    matches!(
        operation_name(operation).as_deref(),
        Some("distinct" | "unique")
    ) && operation_dataset_name(operation)
        .as_deref()
        .is_some_and(|name| find_dataset(datasets, name).is_none())
}

fn is_scope_wide_reference_distinct_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
) -> bool {
    has_oracle_gap_rule_id(rule, "scope_wide_reference_distinct")
        && matches!(
            operation_name(operation).as_deref(),
            Some("distinct" | "unique")
        )
        && operation_dataset_name(operation).is_some()
}

fn is_external_group_date_operation(operation: &OperationSpec, datasets: &[LoadedDataset]) -> bool {
    matches!(
        operation_name(operation).as_deref(),
        Some("min_date" | "max_date")
    ) && operation_dataset_name(operation)
        .as_deref()
        .is_some_and(|name| find_dataset(datasets, name).is_none())
}

fn is_external_group_min_max_operation(
    operation: &OperationSpec,
    datasets: &[LoadedDataset],
) -> bool {
    matches!(operation_name(operation).as_deref(), Some("min" | "max"))
        && operation_dataset_name(operation)
            .as_deref()
            .is_some_and(|name| find_dataset(datasets, name).is_none())
}

pub(crate) fn execute_dataset_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let name = operation_name(operation).unwrap_or_default();
    let operation_dataset = operation_dataset_name(operation);
    if let Some(result) =
        execute_reference_distinct_operation(rule, operation, &name, datasets, all_datasets)
    {
        return result;
    }
    if let Some(result) =
        execute_external_group_alias_operation(rule, operation, &name, datasets, all_datasets)
    {
        return result;
    }
    if let Some(result) =
        execute_external_group_date_operation(rule, operation, &name, datasets, all_datasets)
    {
        return result;
    }
    if let Some(result) =
        execute_external_group_min_max_operation(rule, operation, &name, datasets, all_datasets)
    {
        return result;
    }

    let input = operation_input_datasets(rule, operation, datasets)?;

    let result = match name.as_str() {
        "filter" | "where" | "subset" => {
            let Some(condition_value) = operation_value(
                operation,
                &["where", "condition", "conditions", "check", "filter"],
            ) else {
                return Err(operation_skipped_result(
                    rule,
                    "filter operation is missing a condition",
                ));
            };
            let condition = normalize_condition_value(condition_value)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))?;

            input
                .iter()
                .map(|dataset| {
                    evaluate_condition_group(&condition, dataset)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                        .and_then(|mask| {
                            filter_dataset_by_mask(dataset, &mask).map_err(|source| {
                                operation_skipped_result(rule, source.to_string())
                            })
                        })
                })
                .collect()
        }
        "derive" | "add_column" => {
            let Some(column) =
                string_field(operation, &["target", "as", "output", "column", "name"])
            else {
                return Err(operation_skipped_result(
                    rule,
                    "derive operation is missing a target column",
                ));
            };
            let source_column = string_field(
                operation,
                &[
                    "from",
                    "source_column",
                    "copy_from",
                    "column_ref",
                    "sourceColumn",
                ],
            );
            let expression = string_field(operation, &["expression", "jsonata"]);
            let value = operation_value(operation, &["value", "literal"])
                .cloned()
                .unwrap_or(Value::Null);

            input
                .iter()
                .map(|dataset| {
                    if let Some(source_column) = source_column.as_deref() {
                        derive_column_from_column(dataset, &column, source_column)
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    } else if let Some(expression) = expression.as_deref() {
                        derive_jsonata_column(dataset, &column, expression)
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    } else {
                        derive_literal_column(dataset, &column, &value)
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    }
                })
                .collect()
        }
        "aggregate" | "group_by" | "group_count" | "record_count" => {
            let keys = string_list_field(
                operation,
                &["by", "keys", "group", "group_by", "group_keys"],
            )
            .unwrap_or_default();
            let output = string_field(
                operation,
                &["id", "target", "as", "output", "column", "name"],
            )
            .unwrap_or_else(|| "GROUP_COUNT".to_owned());
            let statistic =
                string_field(operation, &["function", "statistic", "method", "aggregate"])
                    .unwrap_or_else(|| "count".to_owned());
            let source_column = string_field(
                operation,
                &["source_column", "value_column", "measure", "variable"],
            );

            input
                .iter()
                .map(|dataset| {
                    if normalize_operation_key(&statistic) == "count" && source_column.is_none() {
                        group_count_dataset_with_inline_filter(
                            rule, operation, dataset, &keys, &output,
                        )
                    } else {
                        if keys.is_empty() {
                            return Err(operation_skipped_result(
                                rule,
                                "aggregate operation is missing grouping keys",
                            ));
                        }
                        let dataset = apply_operation_inline_filter(rule, operation, dataset)?;
                        group_stat_dataset(
                            &dataset,
                            &keys,
                            source_column.as_deref(),
                            &output,
                            &statistic,
                        )
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    }
                })
                .collect()
        }
        "min" | "max" | "min_date" | "max_date" => {
            let keys = string_list_field(
                operation,
                &["by", "keys", "group", "group_by", "group_keys"],
            )
            .unwrap_or_default();
            let output = string_field(
                operation,
                &["id", "target", "as", "output", "column", "name"],
            )
            .unwrap_or_else(|| format!("${name}"));
            let Some(source_column) = string_field(
                operation,
                &["source_column", "value_column", "measure", "name"],
            ) else {
                return Err(operation_skipped_result(
                    rule,
                    "min/max operation is missing a source variable",
                ));
            };

            if source_column.trim().is_empty() {
                return Err(operation_skipped_result(
                    rule,
                    "min/max operation is missing a source variable",
                ));
            }
            if keys.is_empty() {
                return Err(operation_skipped_result(
                    rule,
                    "min/max operation is missing grouping keys",
                ));
            }

            input
                .iter()
                .map(|dataset| {
                    group_min_max_dataset(
                        dataset,
                        &keys,
                        &source_column,
                        &output,
                        matches!(name.as_str(), "max" | "max_date"),
                    )
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "sort" | "order_by" => {
            let Some(keys) = string_list_field(operation, &["by", "keys", "order_by", "sort_by"])
            else {
                return Err(operation_skipped_result(
                    rule,
                    "sort operation is missing keys",
                ));
            };
            let descending = bool_field(operation, &["descending", "desc"]).unwrap_or_else(|| {
                string_field(operation, &["order", "direction"])
                    .is_some_and(|order| order.eq_ignore_ascii_case("desc"))
            });

            input
                .iter()
                .map(|dataset| {
                    sort_dataset_by_columns(dataset, &keys, descending)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "select" | "keep" | "project" => {
            let Some(columns) =
                string_list_field(operation, &["columns", "variables", "keep", "select"])
            else {
                return Err(operation_skipped_result(
                    rule,
                    "select operation is missing columns",
                ));
            };
            input
                .iter()
                .map(|dataset| {
                    select_dataset_columns(dataset, &columns)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "drop" | "remove_columns" | "exclude_columns" => {
            let Some(columns) =
                string_list_field(operation, &["columns", "variables", "drop", "remove"])
            else {
                return Err(operation_skipped_result(
                    rule,
                    "drop operation is missing columns",
                ));
            };
            input
                .iter()
                .map(|dataset| {
                    drop_dataset_columns(dataset, &columns)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "rename" | "rename_columns" => {
            let Some(renames) = string_map_field(operation, &["columns", "mapping", "renames"])
                .or_else(|| rename_pair(operation))
            else {
                return Err(operation_skipped_result(
                    rule,
                    "rename operation is missing column mapping",
                ));
            };
            input
                .iter()
                .map(|dataset| {
                    rename_dataset_columns(dataset, &renames)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "distinct" | "deduplicate" | "unique" => {
            let keys =
                string_list_field(operation, &["by", "keys", "group", "columns", "variables"])
                    .unwrap_or_default();
            if let (Some(output), Some(source_column)) = (
                string_field(operation, &["id", "target", "as", "output", "column"]),
                string_field(
                    operation,
                    &["source_column", "value_column", "measure", "name"],
                ),
            ) {
                if bool_field(operation, &["value_is_reference"]).unwrap_or(false) {
                    input
                        .iter()
                        .map(|dataset| {
                            derive_reference_distinct_values_dataset(
                                dataset,
                                all_datasets,
                                &source_column,
                                &output,
                            )
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                        })
                        .collect()
                } else {
                    let aliases =
                        string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
                            .unwrap_or_default();
                    input
                        .iter()
                        .map(|dataset| {
                            group_distinct_values_dataset_with_aliases(
                                dataset,
                                &keys,
                                &aliases,
                                &source_column,
                                &output,
                            )
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                        })
                        .collect()
                }
            } else {
                input
                    .iter()
                    .map(|dataset| {
                        deduplicate_dataset_by_columns(dataset, &keys)
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    })
                    .collect()
            }
        }
        "row_number" | "rank" => {
            let column = string_field(operation, &["target", "as", "output", "column", "name"])
                .unwrap_or_else(|| "ROW_NUMBER".to_owned());
            let keys = string_list_field(operation, &["by", "keys", "group_by", "group_keys"])
                .unwrap_or_default();
            input
                .iter()
                .map(|dataset| {
                    row_number_dataset(dataset, &column, &keys)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "domain_label" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$domain_label".to_owned());
            let prefer_domain_name = string_field(operation, &["domain_label_source"])
                .is_some_and(|source| normalize_operation_key(&source) == "domain");
            input
                .iter()
                .map(|dataset| {
                    derive_domain_label_dataset(dataset, &column, prefer_domain_name)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "study_domains" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$study_domains".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_study_domains_dataset(dataset, all_datasets, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "variable_count" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$VARIABLE_COUNT".to_owned());
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
                    "variable_count operation is missing a source variable",
                ));
            };
            input
                .iter()
                .map(|dataset| {
                    derive_variable_count_dataset(dataset, all_datasets, &source_column, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "dy" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$DY".to_owned());
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
                    "dy operation is missing a source date variable",
                ));
            };
            let reference_column = string_field(
                operation,
                &["reference", "reference_column", "ref", "start_date"],
            )
            .unwrap_or_else(|| "RFSTDTC".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_study_day_dataset(dataset, &source_column, &reference_column, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "extract_metadata" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$metadata".to_owned());
            let field = string_field(operation, &["name", "field", "metadata"])
                .unwrap_or_else(|| "dataset_name".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_metadata_dataset(dataset, &field, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "valid_codelist_dates" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$valid_versions".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_valid_codelist_dates_dataset(dataset, operation, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "map" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$mapped".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_mapped_dataset(dataset, operation, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "codelist_extensible" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$codelist_extensible".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_codelist_extensible_dataset(dataset, operation, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "codelist_terms" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$codelist_terms".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_codelist_terms_dataset(dataset, operation, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "split_by" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$split".to_owned());
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
                    "split_by operation is missing a source variable",
                ));
            };
            let delimiter = string_field(operation, &["delimiter", "separator"])
                .unwrap_or_else(|| ",".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_split_by_dataset(dataset, &source_column, &delimiter, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "get_parent_model_column_order" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$model_variables".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_parent_model_column_order_dataset(dataset, &column)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "get_dataset_filtered_variables" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$variables".to_owned());
            let key_name = string_field(operation, &["key_name", "key", "field"])
                .unwrap_or_else(|| "role".to_owned());
            let key_value = string_field(operation, &["key_value", "value"])
                .unwrap_or_else(|| "Timing".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_dataset_filtered_variables_dataset(
                        dataset, &column, &key_name, &key_value,
                    )
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        "get_xhtml_errors" => {
            let column = string_field(operation, &["id", "target", "as", "output", "column"])
                .unwrap_or_else(|| "$xhtml_errors".to_owned());
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
                    "get_xhtml_errors operation is missing a source variable",
                ));
            };
            let namespace = string_field(operation, &["namespace", "xmlns"])
                .unwrap_or_else(|| "http://www.cdisc.org/ns/usdm/xhtml/v1.0".to_owned());
            input
                .iter()
                .map(|dataset| {
                    derive_xhtml_errors_dataset(dataset, &source_column, &column, &namespace)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
        }
        _ => Err(operation_skipped_result(
            rule,
            format!("unsupported operation {name}"),
        )),
    };

    let result = result?;
    match operation_dataset.as_deref() {
        Some(target_dataset) => Ok(merge_operation_target_dataset(
            datasets,
            result,
            target_dataset,
        )),
        None => Ok(result),
    }
}

fn merge_operation_target_dataset(
    datasets: &[LoadedDataset],
    result: Vec<LoadedDataset>,
    target_dataset: &str,
) -> Vec<LoadedDataset> {
    let updates = result
        .iter()
        .filter(|dataset| dataset_matches_name(dataset, target_dataset))
        .collect::<Vec<_>>();
    if updates.is_empty() {
        return result;
    }

    let mut update_index = 0usize;
    let mut merged = Vec::with_capacity(datasets.len());

    for dataset in datasets {
        if dataset_matches_name(dataset, target_dataset) {
            if let Some(updated) = updates.get(update_index) {
                merged.push((*updated).clone());
                update_index += 1;
            } else {
                merged.push(dataset.clone());
            }
        } else {
            merged.push(dataset.clone());
        }
    }

    merged.extend(
        updates
            .iter()
            .skip(update_index)
            .map(|dataset| (*dataset).clone()),
    );
    merged
}

fn execute_external_group_alias_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    name: &str,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> Option<std::result::Result<Vec<LoadedDataset>, RuleValidationResult>> {
    if name != "record_count" {
        return None;
    }
    let source_name = operation_dataset_name(operation)?;
    let scope_wide = is_scope_wide_reference_distinct_operation(rule, operation);
    if find_dataset(datasets, &source_name).is_some() && !scope_wide {
        return None;
    }

    let Some(source_dataset) = find_dataset(all_datasets, &source_name) else {
        return Some(Err(operation_skipped_result(
            rule,
            format!("dataset {source_name} was not available for operation"),
        )));
    };
    let keys = string_list_field(
        operation,
        &["by", "keys", "group", "group_by", "group_keys"],
    )
    .unwrap_or_default();
    let aliases = string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .unwrap_or_default();
    let keys = if rule.entities.is_some() && !aliases.is_empty() && aliases.len() < keys.len() {
        keys.into_iter().take(aliases.len()).collect::<Vec<_>>()
    } else {
        keys
    };
    let output = string_field(
        operation,
        &["id", "target", "as", "output", "column", "name"],
    )
    .unwrap_or_else(|| "GROUP_COUNT".to_owned());

    if keys.is_empty() || aliases.is_empty() || keys.len() != aliases.len() {
        return Some(Err(operation_skipped_result(
            rule,
            "external record_count operation requires matching group and group_aliases",
        )));
    }

    Some(external_record_count_by_group_aliases(
        rule,
        operation,
        source_dataset,
        datasets,
        &keys,
        &aliases,
        &output,
    ))
}

fn external_record_count_by_group_aliases(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    source_dataset: &LoadedDataset,
    target_datasets: &[LoadedDataset],
    keys: &[String],
    aliases: &[String],
    output: &str,
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let source_mask = operation_inline_filter_mask(rule, operation, source_dataset)?;
    let source_key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(source_dataset, key).map_err(|source| {
                operation_skipped_result(rule, format!("source group key {key}: {source}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut counts = BTreeMap::new();
    for row in 0..source_dataset.frame().height() {
        if !source_mask.get(row).copied().unwrap_or(false) {
            continue;
        }
        *counts
            .entry(filtered_group_count_key(&source_key_columns, row, None))
            .or_insert(0_i64) += 1;
    }

    target_datasets
        .iter()
        .map(|dataset| {
            let target_key_columns = aliases
                .iter()
                .map(|alias| {
                    operation_column_values(dataset, alias).map_err(|source| {
                        operation_skipped_result(
                            rule,
                            format!("target group alias {alias}: {source}"),
                        )
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let values = (0..dataset.frame().height())
                .map(|row| {
                    let count = *counts
                        .get(&filtered_group_count_key(&target_key_columns, row, None))
                        .unwrap_or(&0_i64);
                    Value::Number(serde_json::Number::from(count))
                })
                .collect::<Vec<_>>();
            derive_column_from_values_with_aliases(dataset, output, &values)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn execute_external_group_date_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    name: &str,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> Option<std::result::Result<Vec<LoadedDataset>, RuleValidationResult>> {
    if !matches!(name, "min_date" | "max_date") {
        return None;
    }
    let source_name = operation_dataset_name(operation)?;
    if find_dataset(datasets, &source_name).is_some() {
        return None;
    }

    let Some(source_dataset) = find_dataset(all_datasets, &source_name) else {
        return Some(Err(operation_skipped_result(
            rule,
            format!("dataset {source_name} was not available for operation"),
        )));
    };
    let keys = string_list_field(
        operation,
        &["by", "keys", "group", "group_by", "group_keys"],
    )
    .unwrap_or_default();
    let aliases = string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .unwrap_or_else(|| keys.clone());
    let output = string_field(
        operation,
        &["id", "target", "as", "output", "column", "name"],
    )
    .unwrap_or_else(|| format!("${name}"));
    let Some(source_column) = string_field(
        operation,
        &["source_column", "value_column", "measure", "name"],
    ) else {
        return Some(Err(operation_skipped_result(
            rule,
            "date operation is missing a source column",
        )));
    };

    if keys.is_empty() || keys.len() != aliases.len() {
        return Some(Err(operation_skipped_result(
            rule,
            "date operation requires matching group keys and aliases",
        )));
    }

    Some(external_group_date_dataset(
        rule,
        source_dataset,
        datasets,
        &keys,
        &aliases,
        &source_column,
        &output,
        name == "max_date",
    ))
}

#[allow(clippy::too_many_arguments)]
fn external_group_date_dataset(
    rule: &ExecutableRule,
    source_dataset: &LoadedDataset,
    target_datasets: &[LoadedDataset],
    keys: &[String],
    aliases: &[String],
    source_column: &str,
    output: &str,
    choose_max: bool,
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let source_key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(source_dataset, key).map_err(|source| {
                operation_skipped_result(rule, format!("source group key {key}: {source}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let source_dates = operation_column_values(source_dataset, source_column)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mut by_key = BTreeMap::<Vec<String>, String>::new();
    for row in 0..source_dataset.frame().height() {
        let Some(date) = source_dates
            .get(row)
            .and_then(json_scalar_string)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let key = filtered_group_count_key(&source_key_columns, row, None);
        by_key
            .entry(key)
            .and_modify(|current| {
                if (choose_max && date > *current) || (!choose_max && date < *current) {
                    *current = date.clone();
                }
            })
            .or_insert(date);
    }

    target_datasets
        .iter()
        .map(|dataset| {
            let target_key_columns = aliases
                .iter()
                .map(|alias| {
                    operation_column_values(dataset, alias).map_err(|source| {
                        operation_skipped_result(
                            rule,
                            format!("target group alias {alias}: {source}"),
                        )
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let values = (0..dataset.frame().height())
                .map(|row| {
                    by_key
                        .get(&filtered_group_count_key(&target_key_columns, row, None))
                        .map(|value| Value::String(value.clone()))
                        .unwrap_or(Value::Null)
                })
                .collect::<Vec<_>>();
            derive_column_from_values_with_aliases(dataset, output, &values)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn execute_external_group_min_max_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    name: &str,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> Option<std::result::Result<Vec<LoadedDataset>, RuleValidationResult>> {
    if !matches!(name, "min" | "max") {
        return None;
    }
    let source_name = operation_dataset_name(operation)?;
    if find_dataset(datasets, &source_name).is_some() {
        return None;
    }

    let Some(source_dataset) = find_dataset(all_datasets, &source_name) else {
        return Some(Err(operation_skipped_result(
            rule,
            format!("dataset {source_name} was not available for operation"),
        )));
    };
    let keys = string_list_field(
        operation,
        &["by", "keys", "group", "group_by", "group_keys"],
    )
    .unwrap_or_default();
    let aliases = string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .unwrap_or_else(|| keys.clone());
    let output = string_field(
        operation,
        &["id", "target", "as", "output", "column", "name"],
    )
    .unwrap_or_else(|| format!("${name}"));
    let Some(source_column) = string_field(
        operation,
        &["source_column", "value_column", "measure", "name"],
    ) else {
        return Some(Err(operation_skipped_result(
            rule,
            "min/max operation is missing a source column",
        )));
    };

    if keys.is_empty() || keys.len() != aliases.len() {
        return Some(Err(operation_skipped_result(
            rule,
            "min/max operation requires matching group keys and aliases",
        )));
    }

    Some(external_group_min_max_dataset(
        rule,
        source_dataset,
        datasets,
        &keys,
        &aliases,
        &source_column,
        &output,
        name == "max",
    ))
}

#[allow(clippy::too_many_arguments)]
fn external_group_min_max_dataset(
    rule: &ExecutableRule,
    source_dataset: &LoadedDataset,
    target_datasets: &[LoadedDataset],
    keys: &[String],
    aliases: &[String],
    source_column: &str,
    output: &str,
    choose_max: bool,
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let source_key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(source_dataset, key).map_err(|source| {
                operation_skipped_result(rule, format!("source group key {key}: {source}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let source_values = operation_column_values(source_dataset, source_column)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mut by_group = BTreeMap::<Vec<String>, MinMaxValue>::new();
    for row in 0..source_dataset.frame().height() {
        let Some(candidate) = source_values.get(row).and_then(to_min_max_candidate) else {
            continue;
        };
        let key = filtered_group_count_key(&source_key_columns, row, None);
        by_group
            .entry(key)
            .and_modify(|current| {
                let replace = if choose_max {
                    candidate > *current
                } else {
                    candidate < *current
                };
                if replace {
                    *current = candidate.clone();
                }
            })
            .or_insert(candidate);
    }

    target_datasets
        .iter()
        .map(|dataset| {
            let target_key_columns = aliases
                .iter()
                .map(|alias| {
                    operation_column_values(dataset, alias).map_err(|source| {
                        operation_skipped_result(
                            rule,
                            format!("target group alias {alias}: {source}"),
                        )
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let values = (0..dataset.frame().height())
                .map(|row| {
                    by_group
                        .get(&filtered_group_count_key(&target_key_columns, row, None))
                        .map(MinMaxValue::to_json)
                        .unwrap_or(Value::Null)
                })
                .collect::<Vec<_>>();
            derive_column_from_values_with_aliases(dataset, output, &values)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn group_min_max_dataset(
    dataset: &LoadedDataset,
    keys: &[String],
    source_column: &str,
    column_name: &str,
    choose_max: bool,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "min/max operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "min/max operation requires an output column".to_owned(),
        ));
    }
    if keys.is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "min/max operation requires at least one group key".to_owned(),
        ));
    }

    let key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(dataset, key).map_err(|_source| {
                DataError::InvalidDatasetPackage(format!("min/max key not found: {key}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let source_values = operation_column_values(dataset, source_column).map_err(|_source| {
        DataError::InvalidDatasetPackage(format!(
            "min/max source column not found: {source_column}"
        ))
    })?;

    let mut by_group = BTreeMap::<Vec<String>, MinMaxValue>::new();
    for row in 0..dataset.frame().height() {
        let Some(candidate) = source_values.get(row).and_then(to_min_max_candidate) else {
            continue;
        };
        let key = filtered_group_count_key(&key_columns, row, None);
        by_group
            .entry(key)
            .and_modify(|current| {
                let replace = if choose_max {
                    candidate > *current
                } else {
                    candidate < *current
                };
                if replace {
                    *current = candidate.clone();
                }
            })
            .or_insert(candidate);
    }

    let values = (0..dataset.frame().height())
        .map(|row| {
            let key = filtered_group_count_key(&key_columns, row, None);
            by_group.get(&key).map_or(Value::Null, MinMaxValue::to_json)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

#[derive(Clone)]
enum MinMaxValue {
    Number(f64, String),
    Text(String),
}

impl std::cmp::PartialEq for MinMaxValue {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl std::cmp::Eq for MinMaxValue {}

impl std::cmp::PartialOrd for MinMaxValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Ord for MinMaxValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Number(left, _), Self::Number(right, _)) => {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            (Self::Number(_left, left_text), Self::Text(right)) => left_text.cmp(right),
            (Self::Text(left), Self::Number(_right, right_text)) => left.cmp(right_text),
        }
    }
}

impl MinMaxValue {
    fn to_json(&self) -> Value {
        match self {
            Self::Number(value, _) => serde_json::Number::from_f64(*value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::Text(value) => Value::String(value.clone()),
        }
    }
}

fn to_min_max_candidate(value: &Value) -> Option<MinMaxValue> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(MinMaxValue::Text(value.to_string())),
        Value::Number(value) => value
            .as_f64()
            .map(|number| MinMaxValue::Number(number, value.to_string())),
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                None
            } else if let Ok(number) = value.parse::<f64>() {
                Some(MinMaxValue::Number(number, value.to_owned()))
            } else {
                Some(MinMaxValue::Text(value.to_owned()))
            }
        }
        _ => Some(MinMaxValue::Text(value.to_string())),
    }
}

fn execute_reference_distinct_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    name: &str,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> Option<std::result::Result<Vec<LoadedDataset>, RuleValidationResult>> {
    if !is_supported_reference_distinct_rule(rule) {
        return None;
    }

    if !matches!(name, "distinct" | "unique") {
        return None;
    }

    let source_name = operation_dataset_name(operation)?;
    let scope_wide = is_scope_wide_reference_distinct_operation(rule, operation);
    if find_dataset(datasets, &source_name).is_some() && !scope_wide {
        return None;
    }

    let Some(output) = string_field(operation, &["id", "target", "as", "output", "column"]) else {
        return Some(Err(operation_skipped_result(
            rule,
            "reference distinct operation is missing an output column",
        )));
    };
    let Some(source_column) = string_field(
        operation,
        &["source_column", "value_column", "measure", "name"],
    ) else {
        return Some(Err(operation_skipped_result(
            rule,
            "reference distinct operation is missing a source column",
        )));
    };

    let Some(source_dataset) = find_dataset(all_datasets, &source_name) else {
        if is_absent_reference_distinct_source_pass_through_rule(rule, &source_name) {
            return Some(
                datasets
                    .iter()
                    .map(|dataset| {
                        derive_external_distinct_values_dataset_allow_missing_source(
                            dataset,
                            &source_column,
                            &output,
                        )
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    })
                    .collect(),
            );
        }

        return Some(Err(operation_skipped_result(
            rule,
            format!("dataset {source_name} was not loaded"),
        )));
    };

    let group_keys = string_list_field(
        operation,
        &["by", "keys", "group", "group_by", "group_keys"],
    )
    .unwrap_or_default();
    let group_aliases = string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .unwrap_or_default();
    if !group_keys.is_empty() {
        let source_mask = match operation_inline_filter_mask(rule, operation, source_dataset) {
            Ok(mask) => mask,
            Err(skipped) => return Some(Err(skipped)),
        };
        let source_keys = if rule.entities.is_some()
            && !group_aliases.is_empty()
            && group_aliases.len() < group_keys.len()
        {
            group_keys
                .iter()
                .take(group_aliases.len())
                .cloned()
                .collect::<Vec<_>>()
        } else {
            group_keys.clone()
        };
        let target_keys = if group_aliases.is_empty() {
            source_keys.clone()
        } else {
            group_aliases.clone()
        };
        if source_keys.len() != target_keys.len() {
            return Some(Err(operation_skipped_result(
                rule,
                "grouped reference distinct operation requires matching group and group_aliases",
            )));
        }
        return Some(
            datasets
                .iter()
                .filter(|dataset| !scope_wide || !dataset_matches_name(dataset, &source_name))
                .filter(|dataset| {
                    !group_aliases.is_empty()
                        || reference_distinct_target_has_group_keys(dataset, &target_keys)
                })
                .map(|dataset| {
                    derive_external_group_distinct_values_dataset(
                        source_dataset,
                        dataset,
                        &source_mask,
                        &source_keys,
                        &target_keys,
                        &source_column,
                        &output,
                    )
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect(),
        );
    }

    Some(
        datasets
            .iter()
            .filter(|dataset| !scope_wide || !dataset_matches_name(dataset, &source_name))
            .map(|dataset| {
                derive_external_distinct_values_dataset(
                    dataset,
                    source_dataset,
                    &source_column,
                    &output,
                )
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
            })
            .collect(),
    )
}

fn reference_distinct_target_has_group_keys(
    dataset: &LoadedDataset,
    target_keys: &[String],
) -> bool {
    operation_group_key_columns(dataset, target_keys).is_ok()
}

fn is_absent_reference_distinct_source_pass_through_rule(
    rule: &ExecutableRule,
    source_name: &str,
) -> bool {
    engine_semantics::is_absent_reference_distinct_source_pass_through_rule(rule, source_name)
}

fn derive_external_distinct_values_dataset_allow_missing_source(
    dataset: &LoadedDataset,
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if dataset_has_column(dataset, source_column) {
        return derive_external_distinct_values_dataset(
            dataset,
            dataset,
            source_column,
            column_name,
        );
    }

    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(String::new()))
        .collect::<Vec<_>>();
    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_external_distinct_values_dataset(
    dataset: &LoadedDataset,
    source_dataset: &LoadedDataset,
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "reference distinct operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "reference distinct operation requires an output column".to_owned(),
        ));
    }
    let values = operation_column_values(source_dataset, source_column)?;
    let joined = values
        .iter()
        .filter_map(json_distinct_value_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(joined.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_external_group_distinct_values_dataset(
    source_dataset: &LoadedDataset,
    target_dataset: &LoadedDataset,
    source_mask: &[bool],
    source_keys: &[String],
    target_keys: &[String],
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "grouped reference distinct operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "grouped reference distinct operation requires an output column".to_owned(),
        ));
    }
    if source_mask.len() != source_dataset.frame().height() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "filter mask length {} does not match row count {}",
            source_mask.len(),
            source_dataset.frame().height()
        )));
    }

    let source_key_columns = operation_group_key_columns(source_dataset, source_keys)?;
    let source_values = operation_column_values(source_dataset, source_column)?;
    let mut groups = BTreeMap::<Vec<String>, BTreeSet<String>>::new();
    for row in 0..source_dataset.frame().height() {
        if !source_mask.get(row).copied().unwrap_or(false) {
            continue;
        }
        if let Some(value) = source_values.get(row).and_then(json_distinct_value_string) {
            groups
                .entry(filtered_group_count_key(&source_key_columns, row, None))
                .or_default()
                .insert(value);
        }
    }

    let target_key_columns = operation_group_key_columns(target_dataset, target_keys)?;
    let values = (0..target_dataset.frame().height())
        .map(|row| {
            let joined = groups
                .get(&filtered_group_count_key(&target_key_columns, row, None))
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default();
            Value::String(joined)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(target_dataset, column_name, &values)
}

fn derive_reference_distinct_values_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    _source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "reference distinct operation requires an output column".to_owned(),
        ));
    }

    let reference_domains = match dataset_column_values(dataset, "RDOMAIN") {
        Ok(values) => values,
        Err(_) => {
            let values = (0..dataset.summary().row_count)
                .map(|_| Value::String(String::new()))
                .collect::<Vec<_>>();
            return derive_column_from_values_with_aliases(dataset, column_name, &values);
        }
    };
    let values = reference_domains
        .iter()
        .map(|value| {
            let Some(domain) = json_scalar_string(value) else {
                return Value::String(String::new());
            };
            let variable_names = find_dataset(all_datasets, &domain)
                .map(reference_dataset_variable_names)
                .unwrap_or_default();
            Value::String(variable_names.join("|"))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}
