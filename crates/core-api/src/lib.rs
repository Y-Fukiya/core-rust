#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

mod api_types;
mod cdisc_context;
mod condition_inspect;
mod dataset_helpers;
mod domain_presence;
mod engine_semantics;
mod execution_provenance;
mod json_values;
mod match_datasets;
mod metadata_execution;
mod metadata_support;
mod open_rules_compat;
mod operation_columns;
mod operation_datasets;
mod operation_execution;
mod operation_fields;
mod operation_references;
mod operation_runtime;
mod report_variables;
mod result_normalization;
mod result_overrides;
mod rule_preparation;
mod scope_filter;
mod split_domain_unique_set;
mod standard_filter;
mod static_codelists;
mod usdm_hand_ports;

pub use api_types::{
    ApiError, DatasetLoader, Result, RuleSelection, ValidateOutcome, ValidateRequest,
};
pub use open_rules_compat::{
    rule_id_has_oracle_gap_category, rule_id_specific_semantics_classification,
    rule_id_uses_hand_port,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use core_cdisc_library::{load_define_xml_file, ControlledTerm, DefineXmlMetadata};
use core_data::{
    dataset_column_values, derive_column_from_column, derive_column_from_values,
    derive_literal_column, load_datasets_from_paths, load_open_rules_data_dir,
    metadata_row_dataset, metadata_rows_dataset, LoadedDataset,
};
use core_engine::{validate_rule, RuleValidationResult, SkippedReason, ValidationIssue};
use core_report::{write_reports_with_options, ReportMetadata, ReportOptions};
use core_rule_model::{
    load_rules_from_paths, ConditionGroup, ExecutableRule, OperationSpec, Operator, RuleType,
    Sensitivity, ValueExpr,
};
use serde_json::Value;

#[cfg(test)]
use cdisc_context::define_codelist_for_condition;
use cdisc_context::CdiscContext;
pub(crate) use condition_inspect::{
    contains_column_ref_comparator, contains_date_operator,
    contains_domain_placeholder_column_ref_comparator, contains_empty_operator,
    contains_full_regex_wildcard_target, contains_inconsistent_across_dataset_operator,
    contains_longer_than_target, contains_not_unique_relationship_operator,
    contains_presence_operator, contains_sort_operator, contains_target,
    contains_unique_set_operator,
};
pub(crate) use dataset_helpers::{
    dataset_column_name, dataset_domain_value, dataset_has_column, push_unique_string,
};
use dataset_helpers::{dataset_metadata_name, value_is_blank};
use domain_presence::domain_presence_execution_datasets;
use execution_provenance::annotate_results_execution_provenance;
use json_values::json_report_string;
use match_datasets::{
    add_source_row_column, execute_match_datasets, match_dataset_name,
    rule_referenced_columns_with_suffix, rule_references_match_dataset_prefixed_column,
};
use metadata_execution::{
    dataset_domain_values, dataset_variable_names_in_order, expected_model_variables,
    insert_metadata_operation_value, is_custom_domain, model_allowed_variables,
    model_column_order_from_library, model_filtered_variable_names, required_model_variables,
};
use metadata_support::{
    has_dataset_column_order_operation, has_dy_operation, has_expected_variables_operation,
    has_group_date_operation, has_match_dataset_dependent_operation,
    has_model_column_order_operation, has_model_filtered_variables_operation,
    has_reference_distinct_operation, has_required_variables_operation,
    has_unsupported_reference_distinct_operation, has_variable_metadata_domain_prefix_operations,
    is_supported_dataset_metadata_rule, is_supported_value_metadata_rule,
    is_supported_variable_metadata_rule,
};
use open_rules_compat::{
    has_oracle_gap_rule_id, is_dataset_presence_oracle_gap_rule,
    is_distinct_operation_oracle_gap_rule, is_domain_presence_oracle_gap_rule,
    is_dy_operation_oracle_gap_rule, is_known_unsafe_positive_zero_probe_rule,
    is_missing_column_oracle_gap_rule, is_operation_oracle_gap_rule,
    is_required_value_metadata_oracle_gap_rule, is_supported_entity_match_column_ref_rule,
    is_variable_metadata_oracle_gap_rule, post_execution_oracle_gap_result,
    should_defer_entity_column_ref_oracle_gap, should_defer_positive_zero_oracle_gap_probe,
    skipped_oracle_gap_after_operator_checks,
};
pub(crate) use operation_columns::{
    dataset_has_variable, derive_column_from_values_with_aliases, expand_dataset_domain_placeholder,
};
#[cfg(test)]
use operation_datasets::valid_codelist_dates_for_operation;
use operation_fields::{
    bool_field, is_join_operation, operation_name, string_field, string_list_field,
};
use operation_runtime::{
    execute_dataset_operation, execute_join_operation, initial_operation_datasets,
};
use result_normalization::{
    core_000206_idvarval_rdomain_result, core_000677_pooldef_poolid_result,
    core_000744_relrec_faobj_result, core_000757_intervention_relrec_faobj_result,
    core_000884_ts_age_parameter_count_result, dataset_cell_string, normalize_validation_result,
};
#[cfg(test)]
use result_overrides::is_supported_basic_operator;
use result_overrides::{
    core_000095_se_dataset_result, core_000138_dm_dataset_result, core_000466_pp_dataset_result,
    core_000572_cm_dataset_result, entity_column_ref_skipped_result, missing_column_skipped_result,
    missing_scope_wide_reference_target_result, missing_scoped_dataset_presence_result,
    missing_tpt_relationship_pp_dataset_result, missing_tpt_relationship_target_result,
    should_ignore_evaluation_error, skipped_result_for_evaluation_error, unsupported_operation,
    unsupported_operator,
};
use rule_preparation::prepare_rule_for_execution;
use scope_filter::{
    domain_scope_matches, filter_datasets_by_rule_scope, scope_matches, scope_values,
};
use split_domain_unique_set::core_000750_split_domain_unique_set_results;
use standard_filter::{apply_standard_filter, apply_standard_oracle_gap_filter};
#[cfg(test)]
use static_codelists::{
    static_codelist, static_codelist_matches_version, static_codelist_term_by_code,
    static_codelist_term_by_value, static_codelist_term_matches_version, valid_codelist_dates,
};
use usdm_hand_ports::{has_usdm_hand_port_semantics, usdm_hand_port_execution_datasets};

pub fn run_validation(request: ValidateRequest) -> Result<ValidateOutcome> {
    if !request.include_rules.is_empty() && !request.exclude_rules.is_empty() {
        return Err(ApiError::MutuallyExclusiveRuleFilters);
    }
    if request.rule_paths.is_empty() {
        return Err(ApiError::MissingRulePaths);
    }
    if request.dataset_paths.is_empty() {
        return Err(ApiError::MissingDatasetPaths);
    }

    let rules = load_rules_from_paths(&request.rule_paths)?;
    let open_rules_compat = request.open_rules_oracle_compat;
    let mut selection = select_rules(&rules, &request.include_rules, &request.exclude_rules)?;
    apply_standard_filter(
        &mut selection,
        &request.include_rules,
        &request.standard,
        &request.standard_version,
    );
    if open_rules_compat {
        apply_standard_oracle_gap_filter(
            &mut selection,
            &request.standard,
            &request.standard_version,
        );
    }
    let selected_rule_count = selection.selected.len();
    let skipped_selection_count = selection.skipped.len();

    let mut results = selection.skipped.into_iter().collect::<Vec<_>>();
    let mut executable_rules = Vec::new();
    for rule in selection.selected {
        if let Some(skipped) = skipped_unsupported_rule(&rule, open_rules_compat) {
            results.push(skipped);
        } else {
            executable_rules.push(rule);
        }
    }

    let datasets = if executable_rules.is_empty() {
        Vec::new()
    } else {
        load_request_datasets(&request)?
    };
    let cdisc_context = if executable_rules.is_empty() {
        None
    } else {
        Some(CdiscContext::load(
            &request.define_xml_paths,
            &request.ct_paths,
            &request.external_dictionary_paths,
        )?)
    };

    for rule in &executable_rules {
        let cdisc_context = cdisc_context
            .as_ref()
            .ok_or_else(|| ApiError::Internal("missing CDISC context".to_owned()))?;
        let rule = prepare_rule_for_execution(rule, cdisc_context, &request.standard);
        if !open_rules_compat {
            if let Some(result) = core_000206_idvarval_rdomain_result(&rule, &datasets) {
                results.push(result);
                continue;
            }
            if let Some(result) = core_000744_relrec_faobj_result(&rule, &datasets) {
                results.push(result);
                continue;
            }
            if let Some(result) = core_000757_intervention_relrec_faobj_result(&rule, &datasets) {
                results.push(result);
                continue;
            }
        }
        if let Some(result) = core_000677_pooldef_poolid_result(&rule, &datasets) {
            results.push(result);
            continue;
        }
        if let Some(result) = core_000884_ts_age_parameter_count_result(&rule, &datasets) {
            results.push(result);
            continue;
        }
        let execution_datasets = match execution_datasets_for_rule(&rule, &datasets) {
            Ok(datasets) => datasets,
            Err(skipped) => {
                results.push(skipped);
                continue;
            }
        };

        let rule_result_start = results.len();
        for dataset in &execution_datasets {
            let dataset = add_core_000324_missing_cm_dtc(dataset, &rule)?;
            let dataset = add_core_000039_missing_svpresp(&dataset, &rule)?;

            if open_rules_compat
                && is_missing_column_oracle_gap_rule(&rule)
                && !should_defer_positive_zero_oracle_gap_probe(&rule)
                && !engine_semantics::is_missing_column_probe_exception(&rule)
                && contains_missing_target_column(&rule.conditions, &dataset)
            {
                results.push(missing_column_skipped_result(&rule, &dataset));
                continue;
            }

            if open_rules_compat
                && rule.entities.is_some()
                && !is_supported_entity_match_column_ref_rule(&rule)
                && contains_existing_column_ref_comparator(&rule.conditions, &dataset)
                && !should_defer_entity_column_ref_oracle_gap(&rule)
            {
                results.push(entity_column_ref_skipped_result(&rule, &dataset));
                continue;
            }

            if open_rules_compat {
                if let Some(result) = missing_scope_wide_reference_target_result(&rule, &dataset) {
                    results.push(result);
                    continue;
                }

                if let Some(result) = missing_tpt_relationship_target_result(&rule, &dataset) {
                    results.push(result);
                    continue;
                }
            }

            let validation_dataset = add_missing_presence_target_columns(&dataset, &rule)?;
            let validation_dataset = if open_rules_compat {
                add_open_rules_missing_condition_columns(&validation_dataset, &rule)?
            } else {
                validation_dataset
            };
            match validate_rule(&rule, &validation_dataset) {
                Ok(result) => {
                    let result = normalize_validation_result(
                        &rule,
                        &validation_dataset,
                        result,
                        open_rules_compat,
                    );
                    if open_rules_compat {
                        if let Some(skipped) = post_execution_oracle_gap_result(&rule, &result) {
                            results.push(skipped);
                            continue;
                        }
                    }
                    results.push(result);
                }
                Err(source) => {
                    if should_ignore_evaluation_error(
                        &rule,
                        &source,
                        execution_datasets.len(),
                        open_rules_compat,
                    ) {
                        continue;
                    }
                    results.push(skipped_result_for_evaluation_error(
                        &rule,
                        &dataset,
                        source,
                        open_rules_compat,
                    ));
                }
            }
        }

        if let Some(result) = missing_scoped_dataset_presence_result(&rule, &execution_datasets) {
            results.push(result);
        }

        results.extend(core_000750_split_domain_unique_set_results(
            &rule,
            &datasets,
            &results[rule_result_start..],
        ));
        results.extend(core_000878_invalid_condition_context_results(
            &rule,
            &datasets,
            &results[rule_result_start..],
        ));

        if open_rules_compat {
            if let Some(result) = missing_tpt_relationship_pp_dataset_result(
                &rule,
                &execution_datasets,
                &results[rule_result_start..],
            ) {
                results.push(result);
            }

            if let Some(result) =
                core_000138_dm_dataset_result(&rule, &datasets, &results[rule_result_start..])
            {
                results.push(result);
            }

            if let Some(result) =
                core_000095_se_dataset_result(&rule, &datasets, &results[rule_result_start..])
            {
                results.push(result);
            }

            if let Some(result) =
                core_000572_cm_dataset_result(&rule, &datasets, &results[rule_result_start..])
            {
                results.push(result);
            }

            if let Some(result) =
                core_000466_pp_dataset_result(&rule, &datasets, &results[rule_result_start..])
            {
                results.push(result);
            }
        }
    }

    annotate_results_execution_provenance(&mut results);

    let reports = request
        .output_dir
        .map(|output_dir| {
            write_reports_with_options(
                output_dir,
                &results,
                &ReportOptions {
                    output_format: request.output_format,
                    metadata: ReportMetadata {
                        engine_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                        standard: request.standard.clone(),
                        standard_version: request.standard_version.clone(),
                        log_level: request.log_level.clone(),
                        rule_count: Some(selected_rule_count + skipped_selection_count),
                        dataset_count: Some(datasets.len()),
                        define_xml_count: Some(request.define_xml_paths.len()),
                        ct_count: Some(request.ct_paths.len()),
                        external_dictionary_count: Some(request.external_dictionary_paths.len()),
                        ..Default::default()
                    },
                },
            )
        })
        .transpose()?;

    Ok(ValidateOutcome { results, reports })
}

fn load_request_datasets(request: &ValidateRequest) -> Result<Vec<LoadedDataset>> {
    match request.dataset_loader {
        DatasetLoader::Generic => Ok(load_datasets_from_paths(&request.dataset_paths)?),
        DatasetLoader::OpenRulesDataDir => {
            let mut datasets = Vec::new();
            for path in &request.dataset_paths {
                datasets.extend(load_open_rules_data_dir(path)?);
            }
            Ok(datasets)
        }
    }
}

pub fn select_rules(
    rules: &[ExecutableRule],
    include_rules: &[String],
    exclude_rules: &[String],
) -> Result<RuleSelection> {
    if !include_rules.is_empty() && !exclude_rules.is_empty() {
        return Err(ApiError::MutuallyExclusiveRuleFilters);
    }

    let available_ids: BTreeSet<&str> = rules.iter().map(|rule| rule.core_id.as_str()).collect();
    let selected = if include_rules.is_empty() {
        rules
            .iter()
            .filter(|rule| !exclude_rules.iter().any(|id| id == &rule.core_id))
            .cloned()
            .collect()
    } else {
        include_rules
            .iter()
            .filter_map(|id| rules.iter().find(|rule| rule.core_id == *id).cloned())
            .collect()
    };

    let filter_ids = if include_rules.is_empty() {
        exclude_rules
    } else {
        include_rules
    };
    let skipped = missing_rule_ids(filter_ids, &available_ids)
        .into_iter()
        .map(|id| {
            RuleValidationResult::skipped_rule(
                id.clone(),
                SkippedReason::RuleNotFound,
                format!("Requested rule {id} was not found"),
            )
        })
        .collect();

    Ok(RuleSelection { selected, skipped })
}

fn skipped_unsupported_rule(
    rule: &ExecutableRule,
    open_rules_compat: bool,
) -> Option<RuleValidationResult> {
    if !matches!(
        rule.rule_type,
        RuleType::RecordData
            | RuleType::DatasetMetadata
            | RuleType::DomainPresence
            | RuleType::VariableMetadata
            | RuleType::ValueLevelMetadata
            | RuleType::JsonSchema
            | RuleType::Jsonata
    ) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} has unsupported rule type {}",
                rule.core_id,
                rule.rule_type.as_name()
            ),
        ));
    }

    if rule.rule_type == RuleType::DatasetMetadata && !is_supported_dataset_metadata_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} has unsupported dataset metadata semantics",
                rule.core_id
            ),
        ));
    }

    if rule.rule_type == RuleType::VariableMetadata && !is_supported_variable_metadata_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} has unsupported variable metadata semantics",
                rule.core_id
            ),
        ));
    }

    if open_rules_compat && is_required_value_metadata_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses required value dataset metadata oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if rule.rule_type == RuleType::ValueLevelMetadata && !is_supported_value_metadata_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} has unsupported value metadata semantics",
                rule.core_id
            ),
        ));
    }

    if !matches!(
        rule.sensitivity,
        Some(Sensitivity::Record | Sensitivity::Dataset | Sensitivity::Group)
    ) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!("Rule {} has unsupported sensitivity", rule.core_id),
        ));
    }

    if let Some(operation) = unsupported_operation(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses unsupported operation {}",
                rule.core_id, operation
            ),
        ));
    }

    if open_rules_compat {
        if is_known_unsafe_positive_zero_probe_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if should_defer_positive_zero_oracle_gap_probe(rule) {
            return None;
        }

        if is_operation_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses operation oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if is_distinct_operation_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses distinct operation oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if is_dy_operation_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses dy operation oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if is_dataset_presence_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses dataset presence oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if is_domain_presence_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses domain presence oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }

        if is_variable_metadata_oracle_gap_rule(rule) {
            return Some(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses variable metadata oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        }
    }

    if contains_column_ref_comparator(&rule.conditions) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses column-ref comparator semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if open_rules_compat {
        if let Some(skipped) = skipped_oracle_gap_after_operator_checks(rule) {
            return Some(skipped);
        }
    }

    if has_usdm_hand_port_semantics(rule) {
        return None;
    }

    unsupported_operator(&rule.conditions).map(|operator| {
        RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses unsupported operator {}",
                rule.core_id,
                operator.as_name()
            ),
        )
    })
}

fn add_missing_presence_target_columns(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    if !contains_presence_operator(&rule.conditions)
        || rule.rule_type == RuleType::DomainPresence
        || rule
            .datasets
            .as_ref()
            .is_none_or(|datasets| datasets.is_empty())
    {
        return Ok(dataset.clone());
    }

    let mut dataset = dataset.clone();
    for column in presence_target_columns(&rule.conditions) {
        let column = expand_domain_placeholder_for_dataset(&dataset, &column);
        if dataset_has_column(&dataset, &column) {
            continue;
        }
        dataset = derive_literal_column(&dataset, &column, &Value::Null)?;
    }
    Ok(dataset)
}

fn add_open_rules_missing_condition_columns(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    if !should_treat_missing_condition_columns_as_null(rule) {
        return Ok(dataset.clone());
    }

    let mut dataset = dataset.clone();
    for column in condition_target_columns(&rule.conditions) {
        let column = expand_domain_placeholder_for_dataset(&dataset, &column);
        if dataset_has_column(&dataset, &column) {
            continue;
        }
        dataset = derive_literal_column(&dataset, &column, &Value::Null)?;
    }
    Ok(dataset)
}

fn add_core_000324_missing_cm_dtc(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    if !engine_semantics::is_missing_cm_dtc_rule(rule)
        || !dataset_domain_value(dataset).eq_ignore_ascii_case("CM")
        || dataset_has_column(dataset, "CMDTC")
        || !dataset_has_column(dataset, "CMENTPT")
    {
        return Ok(dataset.clone());
    }

    derive_column_from_column(dataset, "CMDTC", "CMENTPT")
}

fn add_core_000039_missing_svpresp(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    if !engine_semantics::assumes_missing_svpresp_is_planned(rule)
        || !dataset_domain_value(dataset).eq_ignore_ascii_case("SV")
        || dataset_has_column(dataset, "SVPRESP")
        || !dataset_has_column(dataset, "VISITNUM")
        || !dataset_has_column(dataset, "VISITDY")
    {
        return Ok(dataset.clone());
    }

    let values = dataset_column_values(dataset, "VISITDY")?
        .into_iter()
        .map(|value| {
            if value_is_blank(&value) {
                Value::String("Y".to_owned())
            } else {
                Value::Null
            }
        })
        .collect::<Vec<_>>();
    derive_column_from_values(dataset, "SVPRESP", &values)
}

fn should_treat_missing_condition_columns_as_null(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "missing_condition_columns_as_null")
}

fn condition_target_columns(group: &ConditionGroup) -> BTreeSet<String> {
    let mut columns = BTreeSet::new();
    collect_condition_target_columns(group, &mut columns);
    columns
}

fn collect_condition_target_columns(group: &ConditionGroup, columns: &mut BTreeSet<String>) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_condition_target_columns(group, columns);
            }
        }
        ConditionGroup::Not(group) => collect_condition_target_columns(group, columns),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = condition.target.as_deref() {
                columns.insert(target.to_owned());
            }
        }
    }
}

pub(crate) fn presence_target_columns(group: &ConditionGroup) -> BTreeSet<String> {
    let mut columns = BTreeSet::new();
    collect_presence_target_columns(group, &mut columns);
    columns
}

fn collect_presence_target_columns(group: &ConditionGroup, columns: &mut BTreeSet<String>) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_presence_target_columns(group, columns);
            }
        }
        ConditionGroup::Not(group) => collect_presence_target_columns(group, columns),
        ConditionGroup::Leaf(condition) => {
            if matches!(condition.operator, Operator::Exists | Operator::NotExists) {
                if let Some(target) = condition.target.as_deref() {
                    columns.insert(target.to_owned());
                }
            }
        }
    }
}

fn contains_existing_column_ref_comparator(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| contains_existing_column_ref_comparator(group, dataset)),
        ConditionGroup::Not(group) => contains_existing_column_ref_comparator(group, dataset),
        ConditionGroup::Leaf(condition) => {
            let ValueExpr::ColumnRef(column) = &condition.comparator else {
                return false;
            };
            let column = expand_domain_placeholder_for_dataset(dataset, column);
            dataset.frame().column(&column).is_ok()
        }
    }
}

fn contains_missing_target_column(group: &ConditionGroup, dataset: &LoadedDataset) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| contains_missing_target_column(group, dataset)),
        ConditionGroup::Not(group) => contains_missing_target_column(group, dataset),
        ConditionGroup::Leaf(condition) => condition.target.as_deref().is_some_and(|target| {
            let target = expand_domain_placeholder_for_dataset(dataset, target);
            !dataset_has_column(dataset, &target)
        }),
    }
}

fn expand_domain_placeholder_for_dataset(dataset: &LoadedDataset, name: &str) -> String {
    let Some(suffix) = name.strip_prefix("--") else {
        return name.to_owned();
    };
    let Some(prefix) = dataset
        .metadata()
        .domain
        .as_deref()
        .filter(|domain| !domain.trim().is_empty())
        .or_else(|| {
            (!dataset.metadata().name.trim().is_empty()).then_some(dataset.metadata().name.as_str())
        })
    else {
        return name.to_owned();
    };
    format!(
        "{}{}",
        prefix.trim().to_ascii_uppercase(),
        suffix.to_ascii_uppercase()
    )
}

fn has_match_dataset_prefixed_column_reference(rule: &ExecutableRule) -> bool {
    rule.datasets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(match_dataset_name)
        .any(|name| rule_references_match_dataset_prefixed_column(rule, &name))
}

fn has_match_dataset_suffixed_column_reference(rule: &ExecutableRule) -> bool {
    rule.datasets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(match_dataset_name)
        .any(|name| !rule_referenced_columns_with_suffix(rule, &name).is_empty())
}

fn is_supported_reference_distinct_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "supported_reference_distinct")
}

pub(crate) fn collect_condition_target_variables(
    group: &ConditionGroup,
    variables: &mut Vec<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_condition_target_variables(group, variables);
            }
        }
        ConditionGroup::Not(group) => collect_condition_target_variables(group, variables),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = &condition.target {
                push_unique_string(variables, target);
            }
        }
    }
}

pub(crate) fn condition_targets_column(group: &ConditionGroup, column: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| condition_targets_column(group, column)),
        ConditionGroup::Not(group) => condition_targets_column(group, column),
        ConditionGroup::Leaf(condition) => condition
            .target
            .as_deref()
            .is_some_and(|target| target.eq_ignore_ascii_case(column)),
    }
}

fn execution_datasets_for_rule(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if let Some(result) = usdm_hand_port_execution_datasets(rule, datasets) {
        return result;
    }

    if rule.rule_type == RuleType::JsonSchema {
        let Some(dataset) = find_dataset(datasets, "JSONSchemaIssue") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires JSONSchemaIssue dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if rule.rule_type == RuleType::DatasetMetadata && is_supported_dataset_metadata_rule(rule) {
        return dataset_metadata_execution_datasets(rule, datasets);
    }

    if rule.rule_type == RuleType::VariableMetadata && is_supported_variable_metadata_rule(rule) {
        return variable_metadata_execution_datasets(rule, datasets);
    }

    if rule.rule_type == RuleType::ValueLevelMetadata && is_supported_value_metadata_rule(rule) {
        return value_metadata_execution_datasets(rule, datasets);
    }

    if rule.rule_type == RuleType::DomainPresence {
        return domain_presence_execution_datasets(rule, datasets);
    }

    if engine_semantics::is_forbidden_send_domain_placeholder_variable_rule(rule) {
        return forbidden_send_domain_placeholder_variable_execution_datasets(rule, datasets);
    }

    let scoped_datasets = filter_datasets_by_rule_scope(rule, datasets);
    if engine_semantics::is_suppae_aesosp_parent_record_rule(rule) {
        return suppae_aesosp_parent_record_execution_datasets(rule, &scoped_datasets, datasets);
    }

    if rule.operations.is_empty() {
        if rule
            .datasets
            .as_ref()
            .is_some_and(|match_datasets| !match_datasets.is_empty())
        {
            return execute_match_datasets(rule, &scoped_datasets, datasets);
        }
        return Ok(scoped_datasets);
    }

    let mut execution_datasets = if (has_dy_operation(rule)
        || has_group_date_operation(rule)
        || has_match_dataset_dependent_operation(rule)
        || has_reference_distinct_operation(rule)
        || has_match_dataset_prefixed_column_reference(rule)
        || has_match_dataset_suffixed_column_reference(rule))
        && rule
            .datasets
            .as_ref()
            .is_some_and(|match_datasets| !match_datasets.is_empty())
    {
        execute_match_datasets(rule, &scoped_datasets, datasets)?
    } else {
        initial_operation_datasets(rule, &scoped_datasets)?
    };
    for operation in &rule.operations {
        if is_join_operation(operation) {
            execution_datasets =
                execute_join_operation(rule, operation, &execution_datasets, datasets)?;
        } else {
            execution_datasets =
                execute_dataset_operation(rule, operation, &execution_datasets, datasets)?;
        }
    }

    Ok(execution_datasets)
}

fn suppae_aesosp_parent_record_execution_datasets(
    rule: &ExecutableRule,
    scoped_datasets: &[LoadedDataset],
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some(parent) = scoped_datasets
        .iter()
        .find(|dataset| dataset_matches_name(dataset, "AE"))
    else {
        return Ok(scoped_datasets.to_vec());
    };
    let parent_with_source = add_source_row_column(parent)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let Some(suppae) = find_dataset(datasets, "SUPPAE") else {
        return metadata_rows_dataset(parent, &[])
            .map(|dataset| vec![dataset])
            .map_err(|source| operation_skipped_result(rule, source.to_string()));
    };

    let qnams = dataset_column_values(suppae, "QNAM").unwrap_or_default();
    let rdomains = dataset_column_values(suppae, "RDOMAIN").unwrap_or_default();
    let usubjids = dataset_column_values(suppae, "USUBJID").unwrap_or_default();
    let idvars = dataset_column_values(suppae, "IDVAR").unwrap_or_default();
    let idvarvals = dataset_column_values(suppae, "IDVARVAL").unwrap_or_default();

    let mut rows = Vec::new();
    for supp_row in 0..suppae.summary().row_count {
        if !dataset_cell_string(&qnams, supp_row).eq_ignore_ascii_case("AESOSP") {
            continue;
        }
        let rdomain = dataset_cell_string(&rdomains, supp_row);
        if !rdomain.trim().is_empty() && !rdomain.eq_ignore_ascii_case("AE") {
            continue;
        }
        let usubjid = dataset_cell_string(&usubjids, supp_row);
        let idvar = dataset_cell_string(&idvars, supp_row);
        let idvarval = dataset_cell_string(&idvarvals, supp_row);
        let Some(parent_row) =
            supp_parent_record_row(&parent_with_source, &usubjid, &idvar, &idvarval)
        else {
            continue;
        };
        let mut row = dataset_row_values(&parent_with_source, parent_row);
        row.insert("QNAM".to_owned(), Value::String("AESOSP".to_owned()));
        rows.push(row);
    }

    metadata_rows_dataset(&parent_with_source, &rows)
        .map(|dataset| vec![dataset])
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

fn supp_parent_record_row(
    parent: &LoadedDataset,
    usubjid: &str,
    idvar: &str,
    idvarval: &str,
) -> Option<usize> {
    if usubjid.trim().is_empty() || idvar.trim().is_empty() || idvarval.trim().is_empty() {
        return None;
    }
    let parent_subjects = dataset_column_values(parent, "USUBJID").ok()?;
    let parent_values = dataset_column_values(parent, idvar.trim()).ok()?;
    (0..parent.summary().row_count).find(|row| {
        dataset_cell_string(&parent_subjects, *row).trim() == usubjid.trim()
            && dataset_cell_string(&parent_values, *row).trim() == idvarval.trim()
    })
}

fn dataset_row_values(dataset: &LoadedDataset, row: usize) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    for column in dataset.summary().columns {
        let value = dataset_column_values(dataset, &column)
            .ok()
            .and_then(|column_values| column_values.get(row).cloned())
            .unwrap_or(Value::Null);
        values.insert(column, value);
    }
    values
}

fn forbidden_send_domain_placeholder_variable_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some(target) = forbidden_send_domain_placeholder_target(rule) else {
        return Ok(Vec::new());
    };

    let mut execution_datasets = Vec::new();
    for dataset in datasets {
        let variable = expand_domain_placeholder_for_dataset(dataset, target);
        if !dataset_has_metadata_variable(dataset, &variable)
            && !dataset_has_column(dataset, &variable)
        {
            continue;
        }

        let mut row = BTreeMap::new();
        row.insert(variable, Value::String(String::new()));
        execution_datasets.push(
            metadata_rows_dataset(dataset, &[row])
                .map_err(|source| operation_skipped_result(rule, source.to_string()))?,
        );
    }
    Ok(execution_datasets)
}

fn forbidden_send_domain_placeholder_target(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        engine_semantics::CORE_000794 => Some("--PTCD"),
        engine_semantics::CORE_000847 => Some("--SCAN"),
        engine_semantics::CORE_000848 => Some("--SCONG"),
        _ => None,
    }
}

fn dataset_has_metadata_variable(dataset: &LoadedDataset, variable: &str) -> bool {
    dataset
        .metadata
        .variables
        .iter()
        .any(|metadata| metadata.name.eq_ignore_ascii_case(variable))
}

fn dataset_metadata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if engine_semantics::is_split_dataset_parent_metadata_rule(rule) {
        return split_dataset_parent_metadata_execution_datasets(rule, datasets);
    }

    let dataset_names = datasets
        .iter()
        .map(dataset_metadata_name)
        .collect::<Vec<_>>()
        .join("|");
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let mut values = BTreeMap::new();
            values.insert(
                "dataset_name".to_owned(),
                Value::String(dataset_metadata_name(dataset)),
            );
            values.insert(
                "dataset_label".to_owned(),
                dataset
                    .metadata
                    .label
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            values.insert(
                "DOMAIN".to_owned(),
                Value::String(dataset_domain_value(dataset)),
            );
            for operation in &rule.operations {
                if operation_name(operation).as_deref() == Some("record_count") {
                    let output = string_field(
                        operation,
                        &["id", "target", "as", "output", "column", "name"],
                    )
                    .unwrap_or_else(|| "$record_count".to_owned());
                    values.insert(
                        output,
                        Value::Number(serde_json::Number::from(dataset.summary().row_count)),
                    );
                }
                if operation_name(operation).as_deref() == Some("dataset_names") {
                    let output = string_field(
                        operation,
                        &["id", "target", "as", "output", "column", "name"],
                    )
                    .unwrap_or_else(|| "$dataset_names".to_owned());
                    values.insert(output, Value::String(dataset_names.clone()));
                }
            }
            metadata_row_dataset(dataset, &values)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn split_dataset_parent_metadata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let dataset_names = datasets
        .iter()
        .map(dataset_metadata_name)
        .collect::<BTreeSet<_>>();
    let dataset_names_value = dataset_names.iter().cloned().collect::<Vec<_>>().join("|");
    let scoped = filter_datasets_by_rule_scope(rule, datasets);
    let mut selected = scoped
        .iter()
        .filter(|dataset| {
            let name = dataset_metadata_name(dataset);
            match rule.core_id.as_str() {
                _ if engine_semantics::is_missing_split_parent_dataset_rule(rule) => {
                    is_missing_split_parent_dataset(&name, &dataset_names)
                }
                _ if engine_semantics::is_missing_findings_about_parent_dataset_rule(rule) => {
                    is_missing_findings_about_parent_dataset(&name, &dataset_names)
                }
                _ => false,
            }
        })
        .collect::<Vec<_>>();

    if selected.is_empty() {
        let representative = scoped
            .iter()
            .find(|dataset| {
                let name = dataset_metadata_name(dataset);
                match rule.core_id.as_str() {
                    _ if engine_semantics::is_missing_split_parent_dataset_rule(rule) => {
                        !(3..=4).contains(&name.len())
                            || name.starts_with("AP")
                            || name.starts_with("FA")
                    }
                    _ if engine_semantics::is_missing_findings_about_parent_dataset_rule(rule) => {
                        !name.starts_with("FA") || name.len() <= 2
                    }
                    _ => true,
                }
            })
            .or_else(|| scoped.first())
            .or_else(|| datasets.first());
        if let Some(dataset) = representative {
            selected.push(dataset);
        }
    }

    selected
        .into_iter()
        .map(|dataset| {
            let mut values = BTreeMap::new();
            values.insert(
                "dataset_name".to_owned(),
                Value::String(dataset_metadata_name(dataset)),
            );
            values.insert(
                "dataset_label".to_owned(),
                dataset
                    .metadata
                    .label
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            values.insert(
                "DOMAIN".to_owned(),
                Value::String(dataset_domain_value(dataset)),
            );
            values.insert(
                "$list_dataset_names".to_owned(),
                Value::String(dataset_names_value.clone()),
            );
            metadata_row_dataset(dataset, &values)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn is_missing_split_parent_dataset(name: &str, dataset_names: &BTreeSet<String>) -> bool {
    let name = name.trim().to_ascii_uppercase();
    if !(3..=4).contains(&name.len()) || name.starts_with("AP") || name.starts_with("FA") {
        return false;
    }
    let parent = &name[..2];
    !dataset_names.contains(parent)
}

fn is_missing_findings_about_parent_dataset(name: &str, dataset_names: &BTreeSet<String>) -> bool {
    let name = name.trim().to_ascii_uppercase();
    if name.len() <= 2 || !name.starts_with("FA") {
        return false;
    }
    let parent = &name[name.len().saturating_sub(2)..];
    !dataset_names.contains(parent)
}

fn resolved_dataset_column_values(dataset: &LoadedDataset, column: &str) -> Option<Vec<Value>> {
    let actual = dataset_column_name(dataset, column)?;
    dataset_column_values(dataset, &actual).ok()
}

fn core_000878_invalid_condition_context_results(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    existing_results: &[RuleValidationResult],
) -> Vec<RuleValidationResult> {
    if rule.core_id != engine_semantics::CORE_000878 {
        return Vec::new();
    }

    let valid_context_ids = datasets
        .iter()
        .flat_map(|dataset| {
            let ids = resolved_dataset_column_values(dataset, "id").unwrap_or_default();
            let instance_types =
                resolved_dataset_column_values(dataset, "instanceType").unwrap_or_default();
            (0..dataset.frame().height()).filter_map(move |row| {
                let id = ids.get(row).map(json_report_string)?;
                let instance_type = instance_types.get(row).map(json_report_string)?;
                matches!(
                    instance_type.as_str(),
                    "Activity" | "ScheduledActivityInstance"
                )
                .then_some(id)
            })
        })
        .collect::<BTreeSet<_>>();

    let existing = existing_results
        .iter()
        .flat_map(|result| result.errors.iter())
        .filter_map(|issue| Some((issue.dataset.to_ascii_uppercase(), issue.row?)))
        .collect::<BTreeSet<_>>();
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let variables = if rule.output_variables.is_empty() {
        vec![
            "$condition_parent_entity".to_owned(),
            "$condition_parent_id".to_owned(),
            "$condition_parent_rel".to_owned(),
            "$condition_rel_type".to_owned(),
            "$condition_name".to_owned(),
            "id".to_owned(),
            "name".to_owned(),
            "parent_id".to_owned(),
            "parent_rel".to_owned(),
            "rel_type".to_owned(),
            "instanceType".to_owned(),
            "value".to_owned(),
            "$error_type".to_owned(),
        ]
    } else {
        rule.output_variables.clone()
    };
    let mut issues_by_dataset = BTreeMap::<String, Vec<ValidationIssue>>::new();
    for dataset in datasets {
        let parent_entities =
            resolved_dataset_column_values(dataset, "parent_entity").unwrap_or_default();
        let parent_rels = resolved_dataset_column_values(dataset, "parent_rel").unwrap_or_default();
        let rel_types = resolved_dataset_column_values(dataset, "rel_type").unwrap_or_default();
        let instance_types =
            resolved_dataset_column_values(dataset, "instanceType").unwrap_or_default();
        let values = resolved_dataset_column_values(dataset, "value").unwrap_or_default();
        let dataset_name = dataset.metadata.name.to_ascii_uppercase();
        for row in 0..dataset.frame().height() {
            let parent_entity = parent_entities
                .get(row)
                .map(json_report_string)
                .unwrap_or_default();
            let parent_rel = parent_rels
                .get(row)
                .map(json_report_string)
                .unwrap_or_default();
            if !parent_entity.eq_ignore_ascii_case("Condition")
                || !parent_rel.eq_ignore_ascii_case("contextIds")
            {
                continue;
            }

            let rel_type = rel_types
                .get(row)
                .map(json_report_string)
                .unwrap_or_default();
            let invalid_context = if rel_type.eq_ignore_ascii_case("reference") {
                let instance_type = instance_types
                    .get(row)
                    .map(json_report_string)
                    .unwrap_or_default();
                !matches!(
                    instance_type.as_str(),
                    "Activity" | "ScheduledActivityInstance"
                )
            } else if rel_type.eq_ignore_ascii_case("definition") {
                let value = values.get(row).map(json_report_string).unwrap_or_default();
                !value.trim().is_empty() && !valid_context_ids.contains(&value)
            } else {
                false
            };
            if !invalid_context {
                continue;
            }

            let row_number = row + 1;
            if existing.contains(&(dataset_name.clone(), row_number)) {
                continue;
            }
            let issue = ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: dataset_name.clone(),
                domain: Some(dataset_domain_value(dataset)),
                row: Some(row_number),
                variables: variables.clone(),
                message: message.clone(),
                usubjid: None,
                seq: None,
            };
            issues_by_dataset
                .entry(issue.dataset.clone())
                .or_default()
                .push(issue);
        }
    }

    issues_by_dataset
        .into_iter()
        .map(|(dataset, errors)| RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset,
            domain: errors.first().and_then(|issue| issue.domain.clone()),
            message: message.clone(),
            error_count: errors.len(),
            errors,
        })
        .collect()
}

fn variable_metadata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if has_dataset_column_order_operation(rule)
        && (has_expected_variables_operation(rule)
            || has_required_variables_operation(rule)
            || has_model_filtered_variables_operation(rule))
    {
        return variable_metadata_operation_execution_datasets(rule, datasets);
    }
    if has_variable_metadata_domain_prefix_operations(rule) {
        return variable_metadata_domain_prefix_execution_datasets(rule, datasets);
    }
    if engine_semantics::is_define_role_metadata_rule(rule) {
        return core_000494_define_role_metadata_datasets(rule, datasets);
    }
    if engine_semantics::is_library_domain_codelist_metadata_rule(rule) {
        return core_000929_domain_codelist_metadata_datasets(rule, datasets);
    }
    if has_model_column_order_operation(rule) {
        return variable_metadata_model_column_order_execution_datasets(rule, datasets);
    }

    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let rows = dataset
                .metadata
                .variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    let mut row = BTreeMap::new();
                    row.insert(
                        "dataset_name".to_owned(),
                        Value::String(dataset_metadata_name(dataset)),
                    );
                    row.insert(
                        "dataset_label".to_owned(),
                        dataset
                            .metadata
                            .label
                            .clone()
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    );
                    row.insert(
                        "variable_name".to_owned(),
                        Value::String(variable.name.clone()),
                    );
                    row.insert(
                        "variable_label".to_owned(),
                        variable
                            .label
                            .clone()
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    );
                    row.insert(
                        "variable_type".to_owned(),
                        variable
                            .variable_type
                            .clone()
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    );
                    row.insert(
                        "variable_length".to_owned(),
                        variable.length.map_or(Value::Null, |length| {
                            Value::Number(serde_json::Number::from(length))
                        }),
                    );
                    row.insert(
                        "variable_order".to_owned(),
                        Value::Number(serde_json::Number::from(index + 1)),
                    );
                    if engine_semantics::uses_library_variable_label_projection(&rule.core_id)
                        || engine_semantics::uses_library_variable_name_projection(&rule.core_id)
                        || engine_semantics::is_define_variable_label_projection_rule(rule)
                    {
                        let domain = dataset_domain_value(dataset);
                        if let Some(library_name) =
                            library_variable_name(&rule.core_id, &domain, &variable.name)
                        {
                            row.insert(
                                "library_variable_name".to_owned(),
                                Value::String(library_name),
                            );
                        }
                        if let Some(library_label) =
                            library_variable_label(&rule.core_id, &domain, &variable.name)
                        {
                            row.insert(
                                "library_variable_label".to_owned(),
                                Value::String(library_label),
                            );
                        }
                        if engine_semantics::is_define_variable_label_projection_rule(rule) {
                            row.insert(
                                "define_variable_name".to_owned(),
                                Value::String(variable.name.clone()),
                            );
                            let variable_label = variable.label.clone().unwrap_or_default();
                            let define_label = if domain.eq_ignore_ascii_case("VS") {
                                define_variable_label_for_dataset_label(
                                    &variable.name,
                                    &variable_label,
                                )
                                .unwrap_or(variable_label)
                            } else {
                                variable_label
                            };
                            row.insert(
                                "define_variable_label".to_owned(),
                                Value::String(define_label),
                            );
                        }
                    }
                    row
                })
                .collect::<Vec<_>>();
            metadata_rows_dataset(dataset, &rows)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn core_000494_define_role_metadata_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let domain = dataset_domain_value(dataset);
            let rows = open_rules_define_xml_for_dataset(dataset)
                .and_then(|define| define_role_metadata_rows(dataset, &define, &domain))
                .unwrap_or_default();
            metadata_rows_dataset(dataset, &rows)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn define_role_metadata_rows(
    dataset: &LoadedDataset,
    define: &DefineXmlMetadata,
    domain: &str,
) -> Option<Vec<BTreeMap<String, Value>>> {
    let define_dataset = define.datasets.iter().find(|define_dataset| {
        define_dataset
            .domain
            .as_deref()
            .or(define_dataset.name.as_deref())
            .is_some_and(|name| name.eq_ignore_ascii_case(domain))
    })?;

    let mut rows = Vec::new();
    for item_ref in &define_dataset.item_refs {
        let Some(item_oid) = item_ref.item_oid.as_deref() else {
            continue;
        };
        let Some(variable) = define
            .variables
            .iter()
            .find(|variable| variable.oid.as_deref() == Some(item_oid))
        else {
            continue;
        };
        let Some(library_role) = library_variable_role(domain, &variable.name) else {
            continue;
        };
        let mut row = BTreeMap::new();
        row.insert(
            "dataset_name".to_owned(),
            Value::String(dataset_metadata_name(dataset)),
        );
        row.insert(
            "define_variable_name".to_owned(),
            Value::String(variable.name.clone()),
        );
        row.insert(
            "library_variable_name".to_owned(),
            Value::String(variable.name.clone()),
        );
        row.insert(
            "define_variable_role".to_owned(),
            item_ref
                .role
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        row.insert(
            "library_variable_role".to_owned(),
            Value::String(library_role),
        );
        let label = dataset
            .metadata
            .variables
            .iter()
            .find(|dataset_variable| dataset_variable.name.eq_ignore_ascii_case(&variable.name))
            .and_then(|dataset_variable| dataset_variable.label.clone())
            .unwrap_or_default();
        row.insert("define_variable_label".to_owned(), Value::String(label));
        rows.push(row);
    }
    Some(rows)
}

fn library_variable_role(domain: &str, variable: &str) -> Option<String> {
    let domain = domain.to_ascii_uppercase();
    let variable = variable.to_ascii_uppercase();
    if matches!(variable.as_str(), "STUDYID" | "DOMAIN" | "USUBJID") {
        return Some("Identifier".to_owned());
    }
    if variable == format!("{domain}SEQ") {
        return Some("Identifier".to_owned());
    }
    if variable == format!("{domain}TESTCD") {
        return Some("Topic".to_owned());
    }
    if variable == format!("{domain}TEST") {
        return Some("Synonym Qualifier".to_owned());
    }
    if matches!(variable.as_str(), "VISITNUM" | "VISIT" | "EPOCH")
        || variable == format!("{domain}DTC")
        || variable == format!("{domain}DY")
    {
        return Some("Timing".to_owned());
    }
    if variable.ends_with("ORRES") || variable.ends_with("STRESC") || variable.ends_with("STRESN") {
        return Some("Result Qualifier".to_owned());
    }
    if variable.ends_with("ORRESU") || variable.ends_with("STRESU") {
        return Some("Variable Qualifier".to_owned());
    }
    Some("Record Qualifier".to_owned())
}

fn core_000929_domain_codelist_metadata_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let domain = dataset_domain_value(dataset);
            let define_projection = define_domain_codelist_projection(dataset, &domain);
            let domain_lib_codes = core_000929_domain_library_codes(dataset);
            let rows = dataset
                .metadata
                .variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    let mut row = BTreeMap::new();
                    row.insert(
                        "dataset_name".to_owned(),
                        Value::String(dataset_metadata_name(dataset)),
                    );
                    row.insert(
                        "variable_name".to_owned(),
                        Value::String(variable.name.clone()),
                    );
                    row.insert(
                        "variable_order".to_owned(),
                        Value::Number(serde_json::Number::from(index + 1)),
                    );
                    row.insert("$domain_is_custom".to_owned(), Value::Bool(false));
                    row.insert(
                        "$domain_lib_ccode".to_owned(),
                        Value::String(domain_lib_codes.clone()),
                    );
                    row.insert(
                        "domain_lib_ccode".to_owned(),
                        Value::String(domain_lib_codes.clone()),
                    );
                    if variable.name.eq_ignore_ascii_case("DOMAIN") {
                        if let Some((define_ccode, define_codes)) = &define_projection {
                            row.insert(
                                "define_variable_ccode".to_owned(),
                                Value::String(define_ccode.clone()),
                            );
                            row.insert(
                                "define_variable_codelist_coded_codes".to_owned(),
                                Value::String(define_codes.clone()),
                            );
                        }
                    }
                    row
                })
                .collect::<Vec<_>>();
            metadata_rows_dataset(dataset, &rows)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn define_domain_codelist_projection(
    dataset: &LoadedDataset,
    domain: &str,
) -> Option<(String, String)> {
    let define = open_rules_define_xml_for_dataset(dataset)?;
    let codelist_oid = define_domain_codelist_oid(&define, domain)?;
    let define_ccode = define
        .codelist_aliases
        .get(&codelist_oid)
        .and_then(|aliases| aliases.iter().find(|alias| alias.as_str() == "C66734"))
        .cloned()
        .unwrap_or_default();
    let define_codes = define
        .codelists
        .iter()
        .filter(|term| term.codelist == codelist_oid)
        .map(define_term_code_or_value)
        .collect::<Vec<_>>()
        .join("|");
    Some((define_ccode, define_codes))
}

fn open_rules_define_xml_for_dataset(dataset: &LoadedDataset) -> Option<DefineXmlMetadata> {
    let data_dir = dataset.metadata.full_path.parent()?;
    let entries = fs::read_dir(data_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().starts_with("define"))
        {
            if let Ok(define) = load_define_xml_file(&path) {
                return Some(define);
            }
        }
    }
    None
}

fn define_domain_codelist_oid(define: &DefineXmlMetadata, domain: &str) -> Option<String> {
    define
        .datasets
        .iter()
        .find(|dataset| {
            dataset
                .domain
                .as_deref()
                .or(dataset.name.as_deref())
                .is_some_and(|name| name.eq_ignore_ascii_case(domain))
        })
        .and_then(|dataset| {
            dataset.item_refs.iter().find_map(|item_ref| {
                let item_oid = item_ref.item_oid.as_deref()?;
                define
                    .variables
                    .iter()
                    .find(|variable| {
                        variable.oid.as_deref() == Some(item_oid)
                            && variable.name.eq_ignore_ascii_case("DOMAIN")
                    })
                    .and_then(|variable| variable.codelist_oid.clone())
            })
        })
        .or_else(|| {
            let expected_oid = format!("IT.{domain}.DOMAIN");
            define
                .variables
                .iter()
                .find(|variable| {
                    variable
                        .oid
                        .as_deref()
                        .is_some_and(|oid| oid.eq_ignore_ascii_case(&expected_oid))
                        && variable.name.eq_ignore_ascii_case("DOMAIN")
                })
                .and_then(|variable| variable.codelist_oid.clone())
        })
}

fn define_term_code_or_value(term: &ControlledTerm) -> String {
    term.code.clone().unwrap_or_else(|| term.value.clone())
}

fn core_000929_domain_library_codes(dataset: &LoadedDataset) -> String {
    let mut codes = vec![
        "C49562", "C49568", "C95087", "C49587", "C85442", "C61536", "C49602", "C49603", "C49604",
        "C49606", "C49608", "C49609", "C49610", "C49615", "C49616", "C49617", "C49618", "C49619",
        "C49620", "C49621", "C49622", "C49592",
    ];
    match open_rules_env_value(dataset, "VERSION").as_deref() {
        Some("3-3") => {
            codes.push("C00003");
            codes.push("C49563");
        }
        Some("3-4") => {
            codes.push("C00003");
        }
        _ => {}
    }
    codes.join("|")
}

fn open_rules_env_value(dataset: &LoadedDataset, key: &str) -> Option<String> {
    let data_dir = dataset.metadata.full_path.parent()?;
    let env = fs::read_to_string(data_dir.join(".env")).ok()?;
    env.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_owned())
    })
}

fn library_variable_name(rule_id: &str, domain: &str, variable: &str) -> Option<String> {
    if engine_semantics::uses_library_variable_label_projection(rule_id)
        && library_variable_label(rule_id, domain, variable).is_some()
    {
        return Some(variable.to_owned());
    }
    if !engine_semantics::uses_library_variable_name_projection(rule_id) {
        return None;
    }

    let variable = variable.to_ascii_uppercase();
    let allowed = match domain.to_ascii_uppercase().as_str() {
        "DM" => [
            "STUDYID", "DOMAIN", "USUBJID", "SUBJID", "RFSTDTC", "RFENDTC", "RFXSTDTC", "RFXENDTC",
            "ARMCD", "ARM", "SETCD",
        ]
        .as_slice(),
        "CO" => ["STUDYID", "DOMAIN", "RDOMAIN", "USUBJID", "POOLID", "COSEQ"].as_slice(),
        "SE" => ["STUDYID", "DOMAIN", "USUBJID", "SESEQ", "ETCD", "ELEMENT"].as_slice(),
        _ => [].as_slice(),
    };

    allowed
        .iter()
        .any(|allowed| *allowed == variable)
        .then_some(variable)
}

fn library_variable_label(rule_id: &str, _domain: &str, variable: &str) -> Option<String> {
    if !engine_semantics::uses_library_variable_label_projection(rule_id) {
        return None;
    }

    match (rule_id, variable.to_ascii_uppercase().as_str()) {
        (engine_semantics::CORE_000398, "AESDTH") => Some("Results in Death".to_owned()),
        (engine_semantics::CORE_000398, "LBMETHOD") => {
            Some("Method of Test or Examination".to_owned())
        }
        (engine_semantics::CORE_000398, "ECROUTE") => Some("Route of Administration".to_owned()),
        _ => None,
    }
}

fn define_variable_label_for_dataset_label(variable: &str, label: &str) -> Option<String> {
    match (variable.to_ascii_uppercase().as_str(), label) {
        ("USUBJID", "Distinct Subject Identifier") => Some("Unique Subject Identifier".to_owned()),
        ("VSTESTCD", "Blabla") => Some("Vital Signs Test Short Name".to_owned()),
        ("VSORRESU", "Original Units as Collected") => Some("Original Units".to_owned()),
        _ => None,
    }
}

fn value_metadata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let mut execution_datasets = Vec::new();
    for dataset in filter_datasets_by_rule_scope(rule, datasets) {
        let frame = dataset.frame();
        for variable in &dataset.metadata.variables {
            let variable_type = variable.variable_type.clone().unwrap_or_default();
            if !variable_type.eq_ignore_ascii_case("Char") {
                continue;
            }
            let column = frame
                .column(&variable.name)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
            let mut rows = Vec::new();
            for row_index in 0..frame.height() {
                let value = column
                    .get(row_index)
                    .map(|value| {
                        value
                            .extract_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    })
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
                let mut row = BTreeMap::new();
                row.insert(
                    "dataset_name".to_owned(),
                    Value::String(dataset_metadata_name(&dataset)),
                );
                row.insert(
                    "variable_data_type".to_owned(),
                    Value::String(variable_type.clone()),
                );
                row.insert(
                    "variable_name".to_owned(),
                    Value::String(variable.name.clone()),
                );
                row.insert("variable_value".to_owned(), Value::String(value));
                rows.push(row);
            }
            execution_datasets.push(
                metadata_rows_dataset(&dataset, &rows)
                    .map_err(|source| operation_skipped_result(rule, source.to_string()))?,
            );
        }
    }
    Ok(execution_datasets)
}

fn variable_metadata_model_column_order_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let allowed_variables =
                if engine_semantics::is_model_column_order_from_library_rule(rule) {
                    model_column_order_from_library(dataset)
                } else {
                    model_allowed_variables(dataset)
                };
            if engine_semantics::is_model_column_order_from_library_rule(rule) {
                let mut row = BTreeMap::new();
                row.insert(
                    "dataset_name".to_owned(),
                    Value::String(dataset_metadata_name(dataset)),
                );
                insert_metadata_operation_value(
                    &mut row,
                    "$column_order_from_dataset",
                    Value::String(dataset_variable_names_in_order(dataset).join("|")),
                );
                insert_metadata_operation_value(
                    &mut row,
                    "$column_order_from_library",
                    Value::String(allowed_variables.join("|")),
                );
                return metadata_rows_dataset(dataset, &[row])
                    .map_err(|source| operation_skipped_result(rule, source.to_string()));
            }
            let rows = dataset
                .metadata
                .variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    let mut row = BTreeMap::new();
                    row.insert(
                        "dataset_name".to_owned(),
                        Value::String(dataset_metadata_name(dataset)),
                    );
                    row.insert(
                        "variable_name".to_owned(),
                        Value::String(variable.name.clone()),
                    );
                    row.insert(
                        "variable_order".to_owned(),
                        Value::Number(serde_json::Number::from(index + 1)),
                    );
                    for operation in &rule.operations {
                        if matches!(
                            operation_name(operation).as_deref(),
                            Some("get_model_column_order" | "get_column_order_from_library")
                        ) {
                            let output = string_field(
                                operation,
                                &["id", "target", "as", "output", "column"],
                            )
                            .unwrap_or_else(|| "$allowed_variables".to_owned());
                            insert_metadata_operation_value(
                                &mut row,
                                &output,
                                Value::String(allowed_variables.join("|")),
                            );
                        }
                    }
                    row
                })
                .collect::<Vec<_>>();
            metadata_rows_dataset(dataset, &rows)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn variable_metadata_domain_prefix_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let domain_values = dataset_domain_values(dataset);
            let domain_is_custom = domain_values.iter().any(|domain| is_custom_domain(domain));
            let domain_list = domain_values.join("|");
            let rows = dataset
                .metadata
                .variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    let mut row = BTreeMap::new();
                    row.insert(
                        "dataset_name".to_owned(),
                        Value::String(dataset_metadata_name(dataset)),
                    );
                    row.insert(
                        "variable_name".to_owned(),
                        Value::String(variable.name.clone()),
                    );
                    row.insert(
                        "variable_order".to_owned(),
                        Value::Number(serde_json::Number::from(index + 1)),
                    );
                    for operation in &rule.operations {
                        let output =
                            string_field(operation, &["id", "target", "as", "output", "column"])
                                .unwrap_or_else(|| {
                                    format!(
                                        "${}",
                                        operation_name(operation)
                                            .unwrap_or_else(|| "operation".to_owned())
                                    )
                                });
                        match operation_name(operation).as_deref() {
                            Some("distinct") => {
                                insert_metadata_operation_value(
                                    &mut row,
                                    &output,
                                    Value::String(domain_list.clone()),
                                );
                            }
                            Some("domain_is_custom") => {
                                insert_metadata_operation_value(
                                    &mut row,
                                    &output,
                                    Value::Bool(domain_is_custom),
                                );
                            }
                            _ => {}
                        }
                    }
                    row
                })
                .collect::<Vec<_>>();
            metadata_rows_dataset(dataset, &rows)
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

fn variable_metadata_operation_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    filter_datasets_by_rule_scope(rule, datasets)
        .iter()
        .map(|dataset| {
            let dataset_variables = dataset_variable_names_in_order(dataset);
            let expected_variables = expected_model_variables(dataset);
            let required_variables = required_model_variables(dataset);
            let mut row = BTreeMap::new();
            row.insert(
                "dataset_name".to_owned(),
                Value::String(dataset_metadata_name(dataset)),
            );
            row.insert(
                "dataset_label".to_owned(),
                dataset
                    .metadata
                    .label
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            row.insert(
                "variable_name".to_owned(),
                Value::String(dataset_variables.join("|")),
            );
            for operation in &rule.operations {
                let output = string_field(operation, &["id", "target", "as", "output", "column"])
                    .unwrap_or_else(|| {
                        format!(
                            "${}",
                            operation_name(operation).unwrap_or_else(|| "variables".to_owned())
                        )
                    });
                match operation_name(operation).as_deref() {
                    Some("expected_variables") => {
                        insert_metadata_operation_value(
                            &mut row,
                            &output,
                            Value::String(expected_variables.join("|")),
                        );
                    }
                    Some("required_variables") => {
                        insert_metadata_operation_value(
                            &mut row,
                            &output,
                            Value::String(required_variables.join("|")),
                        );
                    }
                    Some("get_model_filtered_variables") => {
                        let key_name = string_field(operation, &["key_name", "key", "field"])
                            .unwrap_or_else(|| "role".to_owned());
                        let key_value = string_field(operation, &["key_value", "value"])
                            .unwrap_or_else(|| "Timing".to_owned());
                        insert_metadata_operation_value(
                            &mut row,
                            &output,
                            Value::String(
                                model_filtered_variable_names(dataset, &key_name, &key_value)
                                    .join("|"),
                            ),
                        );
                    }
                    Some("get_column_order_from_dataset") => {
                        insert_metadata_operation_value(
                            &mut row,
                            &output,
                            Value::String(dataset_variables.join("|")),
                        );
                    }
                    _ => {}
                }
            }
            metadata_rows_dataset(dataset, &[row])
                .map_err(|source| operation_skipped_result(rule, source.to_string()))
        })
        .collect()
}

pub(crate) fn operation_skipped_result(
    rule: &ExecutableRule,
    message: impl Into<String>,
) -> RuleValidationResult {
    let message = message.into();
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::OperationsNotSupported,
        format!("Rule {} cannot run operation: {}", rule.core_id, message),
    )
}

pub(crate) fn join_keys(operation: &OperationSpec) -> Option<(Vec<String>, Vec<String>)> {
    let common_keys = string_list_field(
        operation,
        &["by", "keys", "on", "join_keys", "match_keys", "key"],
    );
    let left_keys = string_list_field(
        operation,
        &[
            "left_by",
            "left_keys",
            "left_on",
            "left_key",
            "left_join_keys",
        ],
    )
    .or_else(|| common_keys.clone());
    let right_keys = string_list_field(
        operation,
        &[
            "right_by",
            "right_keys",
            "right_on",
            "right_key",
            "right_join_keys",
        ],
    )
    .or(common_keys);

    left_keys.zip(right_keys)
}

pub(crate) fn join_skipped_result(
    rule: &ExecutableRule,
    message: impl Into<String>,
) -> RuleValidationResult {
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::DatasetJoinNotSupported,
        format!(
            "Rule {} cannot run dataset join: {}",
            rule.core_id,
            message.into()
        ),
    )
}

pub(crate) fn outcome_message(rule: &ExecutableRule) -> Option<String> {
    rule.actions
        .iter()
        .find(|action| action.name == "generate_dataset_error_objects")
        .or_else(|| rule.actions.first())
        .and_then(|action| action.params.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(crate) fn find_dataset<'a>(
    datasets: &'a [LoadedDataset],
    name: &str,
) -> Option<&'a LoadedDataset> {
    datasets
        .iter()
        .find(|dataset| dataset_matches_name(dataset, name))
}

pub(crate) fn dataset_matches_name(dataset: &LoadedDataset, name: &str) -> bool {
    dataset.metadata.name.eq_ignore_ascii_case(name)
        || dataset
            .metadata
            .domain
            .as_deref()
            .is_some_and(|domain| domain_scope_matches(name, domain))
        || dataset.metadata.filename.eq_ignore_ascii_case(name)
}

fn missing_rule_ids<'a>(
    requested: &'a [String],
    available_ids: &BTreeSet<&str>,
) -> Vec<&'a String> {
    let mut seen = BTreeSet::new();
    requested
        .iter()
        .filter(|id| seen.insert(id.as_str()) && !available_ids.contains(id.as_str()))
        .collect()
}

#[cfg(test)]
mod tests;
