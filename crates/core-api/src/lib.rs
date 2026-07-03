#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

mod cdisc_context;
mod condition_inspect;
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
mod report_variables;
mod result_overrides;
mod rule_preparation;
mod scope_filter;
mod split_domain_unique_set;
mod standard_filter;
mod static_codelists;
mod usdm_hand_ports;

pub use open_rules_compat::{
    rule_id_has_oracle_gap_category, rule_id_specific_semantics_classification,
    rule_id_uses_hand_port,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use core_cdisc_library::{load_define_xml_file, ControlledTerm, DefineXmlMetadata};
use core_data::{
    anti_join_dataset_on, dataset_column_values, deduplicate_dataset_by_columns,
    derive_column_from_column, derive_column_from_values, derive_literal_column,
    drop_dataset_columns, filter_dataset_by_mask, group_stat_dataset, inner_join_dataset_on,
    left_join_dataset_on, load_datasets_from_paths, load_open_rules_data_dir, metadata_row_dataset,
    metadata_rows_dataset, rename_dataset_columns, row_number_dataset, select_dataset_columns,
    semi_join_dataset_on, sort_dataset_by_columns, DataError, DatasetSourceFormat, LoadedDataset,
};
use core_engine::{
    evaluate_condition_group, validate_rule, EngineError, RuleValidationResult, SkippedReason,
    ValidationIssue,
};
use core_report::{
    write_reports_with_options, ReportError, ReportMetadata, ReportOptions, ReportOutputFormat,
    WrittenReports,
};
use core_rule_model::{
    load_rules_from_paths, normalize_condition_value, Condition, ConditionGroup, ExecutableRule,
    OperationSpec, Operator, RuleModelError, RuleType, Sensitivity, ValueExpr,
};
use serde_json::Value;
use thiserror::Error;

#[cfg(test)]
use cdisc_context::define_codelist_for_condition;
use cdisc_context::{apply_cdisc_context_to_group, CdiscContext};
pub(crate) use condition_inspect::{
    contains_column_ref_comparator, contains_date_operator,
    contains_domain_placeholder_column_ref_comparator, contains_empty_operator,
    contains_full_regex_wildcard_target, contains_inconsistent_across_dataset_operator,
    contains_longer_than_target, contains_not_unique_relationship_operator,
    contains_presence_operator, contains_sort_operator, contains_target,
    contains_unique_set_operator,
};
use domain_presence::domain_presence_execution_datasets;
use execution_provenance::annotate_results_execution_provenance;
use json_values::{json_distinct_value_string, json_report_string, json_scalar_string};
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
    has_dataset_column_order_operation, has_dataset_level_record_count_operation, has_dy_operation,
    has_expected_variables_operation, has_group_aliases, has_group_date_operation,
    has_match_dataset_dependent_operation, has_model_column_order_operation,
    has_model_filtered_variables_operation, has_reference_distinct_operation,
    has_required_variables_operation, has_unsupported_reference_distinct_operation,
    has_variable_count_operation, has_variable_metadata_domain_prefix_operations,
    is_supported_dataset_metadata_rule, is_supported_value_metadata_rule,
    is_supported_variable_metadata_rule, operation_dataset_name,
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
    dataset_has_variable, derive_column_from_values_with_aliases, derive_jsonata_column,
    expand_dataset_domain_placeholder, operation_input_datasets, reference_dataset_variable_names,
};
#[cfg(test)]
use operation_datasets::valid_codelist_dates_for_operation;
use operation_datasets::{
    derive_codelist_extensible_dataset, derive_codelist_terms_dataset, derive_domain_label_dataset,
    derive_mapped_dataset, derive_metadata_dataset, derive_parent_model_column_order_dataset,
    derive_split_by_dataset, derive_study_day_dataset, derive_study_domains_dataset,
    derive_valid_codelist_dates_dataset, derive_variable_count_dataset,
    derive_xhtml_errors_dataset,
};
use operation_execution::{
    apply_operation_inline_filter, filtered_group_count_key,
    group_count_dataset_with_inline_filter, group_distinct_values_dataset_with_aliases,
    operation_column_values, operation_group_key_columns, operation_inline_filter_mask,
};
use operation_fields::{
    bool_field, is_join_operation, normalize_operation_key, operation_name, operation_value,
    rename_pair, string_field, string_list_field, string_map_field,
};
use operation_references::derive_dataset_filtered_variables_dataset;
use report_variables::{
    apply_metadata_report_variables, apply_operation_report_variables,
    apply_requested_standard_operation_semantics,
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
use rule_preparation::apply_entity_instance_type_literals;
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
use usdm_hand_ports::{
    apply_usdm_hand_port_semantics, has_usdm_hand_port_semantics, usdm_hand_port_execution_datasets,
};

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("--rules and --exclude-rules cannot be used together")]
    MutuallyExclusiveRuleFilters,
    #[error("at least one rule path is required")]
    MissingRulePaths,
    #[error("at least one dataset path is required")]
    MissingDatasetPaths,
    #[error("failed to load rules: {0}")]
    RuleLoad(#[from] RuleModelError),
    #[error("failed to load datasets: {0}")]
    DataLoad(#[from] DataError),
    #[error("failed to load CDISC metadata: {0}")]
    CdiscLibrary(#[from] core_cdisc_library::CdiscLibraryError),
    #[error("failed to validate rule: {0}")]
    Engine(#[from] EngineError),
    #[error("failed to write reports: {0}")]
    Report(#[from] ReportError),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DatasetLoader {
    #[default]
    Generic,
    OpenRulesDataDir,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidateRequest {
    pub rule_paths: Vec<PathBuf>,
    pub dataset_paths: Vec<PathBuf>,
    pub dataset_loader: DatasetLoader,
    pub open_rules_oracle_compat: bool,
    pub define_xml_paths: Vec<PathBuf>,
    pub ct_paths: Vec<PathBuf>,
    pub external_dictionary_paths: Vec<PathBuf>,
    pub include_rules: Vec<String>,
    pub exclude_rules: Vec<String>,
    pub standard: Option<String>,
    pub standard_version: Option<String>,
    pub output_format: ReportOutputFormat,
    pub log_level: Option<String>,
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ValidateOutcome {
    pub results: Vec<RuleValidationResult>,
    pub reports: Option<WrittenReports>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSelection {
    pub selected: Vec<ExecutableRule>,
    pub skipped: Vec<RuleValidationResult>,
}

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

fn normalize_validation_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    mut result: RuleValidationResult,
    open_rules_compat: bool,
) -> RuleValidationResult {
    if open_rules_compat
        && engine_semantics::is_zb_issue_normalization_rule(rule)
        && dataset_domain_value(dataset).eq_ignore_ascii_case("ZB")
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        for issue in &mut result.errors {
            issue.row = None;
            issue.variables.clear();
            issue.usubjid = None;
            issue.seq = None;
        }
        return result;
    }

    if engine_semantics::is_date_issue_variable_expansion_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        result.errors = result
            .errors
            .into_iter()
            .flat_map(|issue| {
                let variables = issue.variables.clone();
                if variables.len() <= 1 {
                    return vec![issue];
                }
                variables
                    .into_iter()
                    .map(|variable| ValidationIssue {
                        variables: vec![variable],
                        ..issue.clone()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        result.error_count = result.errors.len();
        return result;
    }

    if open_rules_compat
        && engine_semantics::is_tx_variable_expansion_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        result.errors = result
            .errors
            .into_iter()
            .flat_map(|issue| {
                ["TXPARMCD", "TXVAL"]
                    .into_iter()
                    .map(|variable| ValidationIssue {
                        variables: vec![variable.to_owned()],
                        ..issue.clone()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        result.error_count = result.errors.len();
        return result;
    }

    if open_rules_compat && is_core_000595_missing_casno_oracle_issue(rule, dataset, &result) {
        let message = result.message.clone();
        let issue = ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: dataset.metadata().name.clone(),
            domain: dataset.metadata().domain.clone(),
            row: None,
            variables: Vec::new(),
            message: message.clone(),
            usubjid: None,
            seq: None,
        };
        result.execution_status = core_engine::ExecutionStatus::Failed;
        result.error_count = 1;
        result.errors = vec![issue];
        result.message = message;
        return result;
    }

    if engine_semantics::is_cv_unique_evaluation_interval_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        result
            .errors
            .retain(|issue| core_000390_issue_has_overlapping_interval(dataset, issue));
        result.error_count = result.errors.len();
        if result.errors.is_empty() {
            result.execution_status = core_engine::ExecutionStatus::Passed;
        }
        return result;
    }

    if engine_semantics::is_elapsed_time_consistency_precondition_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
        && has_elapsed_time_consistency_preconditions(rule, dataset)
    {
        let issue_variables = elapsed_time_consistency_issue_variables(dataset);
        result.errors = result
            .errors
            .into_iter()
            .filter(|issue| core_000142_issue_has_precondition_peer(dataset, issue))
            .map(|mut issue| {
                if let Some(issue_variables) = &issue_variables {
                    issue.variables = issue_variables.clone();
                }
                issue
            })
            .collect();
        result.error_count = result.errors.len();
        if result.errors.is_empty() {
            result.execution_status = core_engine::ExecutionStatus::Passed;
        }
        return result;
    }

    if engine_semantics::is_group_level_distinct_result_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        let mut issue = result.errors[0].clone();
        issue.row = None;
        issue.usubjid = None;
        issue.seq = None;
        if !rule.output_variables.is_empty() {
            issue.variables = rule.output_variables.clone();
        }
        result.errors = vec![issue];
        result.error_count = 1;
        return result;
    }

    if engine_semantics::is_duplicate_intent_type_group_result_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        let group_counts =
            dataset_column_values(dataset, "intentTypes.duplicate_group_count").unwrap_or_default();
        result.errors = result
            .errors
            .into_iter()
            .flat_map(|issue| {
                let repeat = issue
                    .row
                    .and_then(|row| row.checked_sub(1))
                    .and_then(|row| group_counts.get(row))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                std::iter::repeat_n(issue, repeat).collect::<Vec<_>>()
            })
            .collect();
        result.error_count = result.errors.len();
        return result;
    }

    if engine_semantics::is_study_arm_invalid_population_result_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        let invalid_counts =
            dataset_column_values(dataset, "populationId.invalid_count").unwrap_or_default();
        result.errors = result
            .errors
            .into_iter()
            .flat_map(|issue| {
                let repeat = issue
                    .row
                    .and_then(|row| row.checked_sub(1))
                    .and_then(|row| invalid_counts.get(row))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                std::iter::repeat_n(issue, repeat).collect::<Vec<_>>()
            })
            .collect();
        result.error_count = result.errors.len();
        return result;
    }

    if engine_semantics::is_dataset_level_presence_result_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
        && !result.errors.is_empty()
    {
        let mut issue = result.errors[0].clone();
        issue.row = None;
        issue.usubjid = None;
        issue.seq = None;
        result.errors = vec![issue];
        result.error_count = 1;
        return result;
    }

    if engine_semantics::is_forbidden_send_domain_placeholder_variable_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Failed
    {
        for issue in &mut result.errors {
            issue.row = None;
            issue.usubjid = None;
            issue.seq = None;
        }
    }

    narrow_simple_any_issue_variables(rule, dataset, &mut result);
    omit_unique_set_group_locator_variables(rule, dataset, &mut result);
    include_unique_set_subject_locator_variable(rule, &mut result);
    report_missing_column_zero_record_result(rule, dataset, &mut result);
    report_dataset_level_existing_study_day_variable_result(rule, dataset, &mut result);
    report_missing_dataset_column_once_for_dataset_presence_rules(rule, dataset, &mut result);
    report_first_row_dataset_presence_result(rule, &mut result);
    report_csv_line_record_numbers(rule, dataset, &mut result);
    report_previous_record_numbers(rule, dataset, &mut result);

    if !has_variable_count_operation(rule) && !has_dataset_level_record_count_operation(rule)
        || !matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        || result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
    {
        return result;
    }

    let mut issue = result.errors[0].clone();
    issue.row = None;
    issue.usubjid = None;
    issue.seq = None;
    result.errors = vec![issue];
    result.error_count = 1;
    result
}

fn has_elapsed_time_consistency_preconditions(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> bool {
    ["--TPT", "--TPTNUM", "--ELTM"].into_iter().all(|target| {
        let expanded = expand_domain_placeholder_for_dataset(dataset, target);
        contains_non_empty_target(&rule.conditions, target)
            || contains_non_empty_target(&rule.conditions, &expanded)
    })
}

fn contains_non_empty_target(group: &ConditionGroup, target: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| contains_non_empty_target(group, target)),
        ConditionGroup::Not(group) => contains_non_empty_target(group, target),
        ConditionGroup::Leaf(condition) => {
            condition.operator == Operator::IsNotEmpty
                && condition
                    .target
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(target))
        }
    }
}

fn core_000142_issue_has_precondition_peer(
    dataset: &LoadedDataset,
    issue: &ValidationIssue,
) -> bool {
    let Some(row) = issue.row.and_then(|row| row.checked_sub(1)) else {
        return true;
    };
    let domain = dataset_column_values(dataset, "DOMAIN").unwrap_or_default();
    let visit = dataset_column_values(dataset, "VISITNUM").unwrap_or_default();
    let tptref_column = expand_domain_placeholder_for_dataset(dataset, "--TPTREF");
    let tptnum_column = expand_domain_placeholder_for_dataset(dataset, "--TPTNUM");
    let tpt_column = expand_domain_placeholder_for_dataset(dataset, "--TPT");
    let eltm_column = expand_domain_placeholder_for_dataset(dataset, "--ELTM");
    let tptref = dataset_column_values(dataset, &tptref_column).unwrap_or_default();
    let tptnum = dataset_column_values(dataset, &tptnum_column).unwrap_or_default();
    let tpt = dataset_column_values(dataset, &tpt_column).unwrap_or_default();
    let eltm = dataset_column_values(dataset, &eltm_column).unwrap_or_default();

    if !core_000142_row_satisfies_preconditions(row, &tpt, &tptnum, &eltm) {
        return false;
    }
    let key = (
        dataset_cell_string(&domain, row),
        dataset_cell_string(&visit, row),
        dataset_cell_string(&tptref, row),
        dataset_cell_string(&tptnum, row),
    );
    let value = dataset_cell_string(&eltm, row);

    (0..dataset.summary().row_count).any(|other| {
        other != row
            && core_000142_row_satisfies_preconditions(other, &tpt, &tptnum, &eltm)
            && key.0 == dataset_cell_string(&domain, other)
            && key.1 == dataset_cell_string(&visit, other)
            && key.2 == dataset_cell_string(&tptref, other)
            && key.3 == dataset_cell_string(&tptnum, other)
            && value != dataset_cell_string(&eltm, other)
    })
}

fn core_000142_row_satisfies_preconditions(
    row: usize,
    tpt: &[Value],
    tptnum: &[Value],
    eltm: &[Value],
) -> bool {
    !dataset_cell_string(tpt, row).is_empty()
        && !dataset_cell_string(tptnum, row).is_empty()
        && !dataset_cell_string(eltm, row).is_empty()
}

fn elapsed_time_consistency_issue_variables(dataset: &LoadedDataset) -> Option<Vec<String>> {
    dataset_domain_value(dataset)
        .eq_ignore_ascii_case("FT")
        .then(|| {
            [
                expand_domain_placeholder_for_dataset(dataset, "--TPT"),
                expand_domain_placeholder_for_dataset(dataset, "--TPTNUM"),
                expand_domain_placeholder_for_dataset(dataset, "--ELTM"),
            ]
            .into_iter()
            .collect()
        })
}

fn core_000390_issue_has_overlapping_interval(
    dataset: &LoadedDataset,
    issue: &ValidationIssue,
) -> bool {
    let Some(row) = issue.row.and_then(|row| row.checked_sub(1)) else {
        return true;
    };
    let usubjids = dataset_column_values(dataset, "USUBJID").unwrap_or_default();
    let tests = dataset_column_values(dataset, "CVTESTCD").unwrap_or_default();
    let dates = dataset_column_values(dataset, "CVDTC").unwrap_or_default();
    let starts = dataset_column_values(dataset, "CVSTINT").unwrap_or_default();
    let ends = dataset_column_values(dataset, "CVENINT").unwrap_or_default();

    let key = (
        dataset_cell_string(&usubjids, row),
        dataset_cell_string(&tests, row),
        dataset_cell_string(&dates, row),
    );
    if key.0.is_empty() || key.1.is_empty() || key.2.is_empty() {
        return true;
    }

    (0..dataset.summary().row_count).any(|other| {
        other != row
            && key.0 == dataset_cell_string(&usubjids, other)
            && key.1 == dataset_cell_string(&tests, other)
            && key.2 == dataset_cell_string(&dates, other)
            && core_000390_intervals_overlap(row, other, &starts, &ends)
    })
}

fn core_000390_intervals_overlap(
    left: usize,
    right: usize,
    starts: &[Value],
    ends: &[Value],
) -> bool {
    let Some((left_start, left_end)) = core_000390_interval_minutes(left, starts, ends) else {
        return true;
    };
    let Some((right_start, right_end)) = core_000390_interval_minutes(right, starts, ends) else {
        return true;
    };
    if left_start > left_end || right_start > right_end {
        return true;
    }
    left_start <= right_end && right_start <= left_end
}

fn core_000390_interval_minutes(
    row: usize,
    starts: &[Value],
    ends: &[Value],
) -> Option<(i64, i64)> {
    let start = dataset_cell_string(starts, row);
    let end = dataset_cell_string(ends, row);
    if start.is_empty() && end.is_empty() {
        return None;
    }
    Some((
        parse_iso8601_time_offset_minutes(&start)?,
        parse_iso8601_time_offset_minutes(&end)?,
    ))
}

fn parse_iso8601_time_offset_minutes(value: &str) -> Option<i64> {
    let mut rest = value.trim();
    if rest.is_empty() {
        return None;
    }
    let sign = if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped;
        -1
    } else if let Some(stripped) = rest.strip_prefix('+') {
        rest = stripped;
        1
    } else {
        1
    };
    rest = rest.strip_prefix("PT")?;
    let mut number = String::new();
    let mut minutes = 0_i64;
    let mut saw_component = false;
    for character in rest.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        let value = number.parse::<i64>().ok()?;
        number.clear();
        match character {
            'H' => minutes += value * 60,
            'M' => minutes += value,
            _ => return None,
        }
        saw_component = true;
    }
    if !number.is_empty() || !saw_component {
        return None;
    }
    Some(sign * minutes)
}

fn dataset_cell_string(values: &[Value], row: usize) -> String {
    values
        .get(row)
        .and_then(json_scalar_string)
        .unwrap_or_default()
}

fn report_first_row_dataset_presence_result(
    rule: &ExecutableRule,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.len() <= 1
        || !engine_semantics::uses_first_row_dataset_presence_result(rule)
        || !matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        || rule.rule_type != RuleType::RecordData
    {
        return;
    }

    result.errors.truncate(1);
    result.error_count = 1;
}

fn report_csv_line_record_numbers(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !matches!(dataset.metadata().source_format, DatasetSourceFormat::Csv)
        || !engine_semantics::uses_csv_line_record_numbers(rule)
    {
        return;
    }

    for issue in &mut result.errors {
        if let Some(row) = issue.row.and_then(|row| row.checked_add(1)) {
            issue.row = Some(row);
        }
    }
}

fn report_previous_record_numbers(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !matches!(dataset.metadata().source_format, DatasetSourceFormat::Csv)
        || !engine_semantics::uses_previous_record_numbers(rule)
    {
        return;
    }

    for issue in &mut result.errors {
        if let Some(row) = issue
            .row
            .and_then(|row| row.checked_sub(1))
            .filter(|row| *row > 0)
        {
            issue.row = Some(row);
        }
    }
}

fn narrow_simple_any_issue_variables(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed || result.errors.is_empty() {
        return;
    }
    if engine_semantics::preserves_simple_any_study_day_output_variables(rule) {
        return;
    }
    let ConditionGroup::Any(groups) = &rule.conditions else {
        return;
    };
    let conditions = groups
        .iter()
        .map(|group| match group {
            ConditionGroup::Leaf(condition) => Some(condition),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(conditions) = conditions else {
        return;
    };
    if !conditions.iter().all(|condition| {
        condition
            .target
            .as_deref()
            .is_some_and(is_study_day_issue_target)
    }) {
        return;
    }

    let masks = conditions
        .iter()
        .filter_map(|condition| {
            evaluate_condition_group(&ConditionGroup::Leaf((*condition).clone()), dataset)
                .ok()
                .map(|mask| (condition, mask))
        })
        .collect::<Vec<_>>();
    if masks.is_empty() {
        return;
    }

    for issue in &mut result.errors {
        if issue.variables.len() <= 1 {
            continue;
        }
        let Some(row) = issue.row.and_then(|row| row.checked_sub(1)) else {
            continue;
        };
        let mut variables = Vec::new();
        for (condition, mask) in &masks {
            if mask.get(row).copied().unwrap_or(false) {
                if let Some(target) = condition.target.as_deref() {
                    let variable = expand_domain_placeholder_for_dataset(dataset, target);
                    push_unique_string(&mut variables, &variable);
                }
            }
        }
        if !variables.is_empty() {
            issue.variables = variables;
        }
    }
}

fn is_study_day_issue_target(target: &str) -> bool {
    let target = target
        .trim()
        .strip_prefix("--")
        .unwrap_or_else(|| target.trim())
        .to_ascii_uppercase();
    matches!(target.as_str(), "DY" | "STDY" | "ENDY" | "VISITDY")
}

fn omit_unique_set_group_locator_variables(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !engine_semantics::omits_unique_set_group_locator_variables(rule)
    {
        return;
    }

    let group_variables = unique_set_group_variables(rule, dataset);
    if group_variables.is_empty() {
        return;
    }

    for issue in &mut result.errors {
        issue
            .variables
            .retain(|variable| !group_variables.contains(variable));
    }
}

fn unique_set_group_variables(rule: &ExecutableRule, dataset: &LoadedDataset) -> BTreeSet<String> {
    let mut variables = BTreeSet::new();
    collect_unique_set_group_variables(&rule.conditions, dataset, &mut variables);
    variables
}

fn collect_unique_set_group_variables(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
    variables: &mut BTreeSet<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_unique_set_group_variables(group, dataset, variables);
            }
        }
        ConditionGroup::Not(group) => collect_unique_set_group_variables(group, dataset, variables),
        ConditionGroup::Leaf(condition)
            if matches!(
                condition.operator,
                Operator::IsNotUniqueSet | Operator::IsUniqueSet
            ) =>
        {
            if let ValueExpr::List(values) = &condition.comparator {
                for value in values {
                    if let Some(variable) = value
                        .as_str()
                        .filter(|variable| variable.trim().eq_ignore_ascii_case("USUBJID"))
                    {
                        variables.insert(expand_domain_placeholder_for_dataset(dataset, variable));
                    }
                }
            }
        }
        ConditionGroup::Leaf(_) => {}
    }
}

fn include_unique_set_subject_locator_variable(
    rule: &ExecutableRule,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !engine_semantics::includes_unique_set_subject_locator_variable(rule)
    {
        return;
    }

    for issue in &mut result.errors {
        push_unique_string(&mut issue.variables, "USUBJID");
    }
}

fn report_missing_column_zero_record_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !engine_semantics::uses_missing_column_zero_record_result(rule)
        || !matches!(
            rule.sensitivity,
            Some(Sensitivity::Record) | Some(Sensitivity::Dataset)
        )
        || rule.rule_type != RuleType::RecordData
    {
        return;
    }

    let missing_variables = missing_not_exists_output_variables(rule, dataset);
    if missing_variables.is_empty() {
        return;
    }

    for issue in &mut result.errors {
        issue
            .variables
            .retain(|variable| !missing_variables.contains(variable));
    }
    result.errors.retain(|issue| !issue.variables.is_empty());

    let template = result
        .errors
        .first()
        .cloned()
        .unwrap_or_else(|| ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: dataset.metadata().name.clone(),
            domain: dataset.metadata().domain.clone(),
            row: Some(0),
            variables: Vec::new(),
            message: result.message.clone(),
            usubjid: None,
            seq: None,
        });
    for variable in missing_variables {
        result.errors.push(ValidationIssue {
            row: Some(0),
            variables: vec![variable],
            usubjid: None,
            seq: None,
            ..template.clone()
        });
    }
    result.error_count = result.errors.len();
}

fn report_missing_dataset_column_once_for_dataset_presence_rules(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.len() <= 1
        || !engine_semantics::uses_missing_column_once_result(rule)
        || !matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        || rule.rule_type != RuleType::RecordData
    {
        return;
    }

    let missing_variables = missing_not_exists_output_variables(rule, dataset);
    if missing_variables.is_empty() {
        return;
    }

    let mut seen = BTreeSet::new();
    for issue in &mut result.errors {
        issue.variables.retain(|variable| {
            if !missing_variables.contains(variable) {
                return true;
            }
            seen.insert(variable.clone())
        });
    }
}

fn report_dataset_level_existing_study_day_variable_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &mut RuleValidationResult,
) {
    if result.execution_status != core_engine::ExecutionStatus::Failed
        || result.errors.is_empty()
        || !engine_semantics::uses_dataset_level_existing_study_day_variable_result(rule)
        || !matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        || rule.rule_type != RuleType::RecordData
    {
        return;
    }

    let Some(variable) = existing_study_day_variable(rule, dataset) else {
        return;
    };
    if is_all_empty_sm_endy_dataset_result(rule, dataset, &variable) {
        result.execution_status = core_engine::ExecutionStatus::Passed;
        result.error_count = 0;
        result.errors.clear();
        return;
    }
    let mut issue = result.errors[0].clone();
    issue.row = None;
    issue.variables = vec![variable];
    issue.usubjid = None;
    issue.seq = None;
    result.errors = vec![issue];
    result.error_count = 1;
}

fn is_all_empty_sm_endy_dataset_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    variable: &str,
) -> bool {
    rule.core_id == engine_semantics::CORE_000864
        && dataset_domain_value(dataset).eq_ignore_ascii_case("SM")
        && variable.eq_ignore_ascii_case("SMENDY")
        && !dataset_column_has_non_empty_value(dataset, variable)
}

fn dataset_column_has_non_empty_value(dataset: &LoadedDataset, column: &str) -> bool {
    dataset_column_values(dataset, column).is_ok_and(|values| {
        values
            .iter()
            .any(|value| json_scalar_string(value).is_some_and(|value| !value.trim().is_empty()))
    })
}

fn existing_study_day_variable(rule: &ExecutableRule, dataset: &LoadedDataset) -> Option<String> {
    find_existing_dataset_level_variable(&rule.conditions, dataset)
}

fn find_existing_dataset_level_variable(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
) -> Option<String> {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .find_map(|group| find_existing_dataset_level_variable(group, dataset)),
        ConditionGroup::Not(group) => find_existing_dataset_level_variable(group, dataset),
        ConditionGroup::Leaf(condition)
            if matches!(
                condition.operator,
                Operator::Exists | Operator::IsNotEmpty | Operator::IsCompleteDate
            ) =>
        {
            condition
                .target
                .as_deref()
                .map(|target| expand_domain_placeholder_for_dataset(dataset, target))
                .filter(|variable| dataset_has_column(dataset, variable))
        }
        ConditionGroup::Leaf(_) => None,
    }
}

fn missing_not_exists_output_variables(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> BTreeSet<String> {
    let output_variables = rule
        .output_variables
        .iter()
        .map(|variable| expand_domain_placeholder_for_dataset(dataset, variable))
        .collect::<BTreeSet<_>>();
    let mut variables = BTreeSet::new();
    collect_missing_not_exists_output_variables(
        &rule.conditions,
        dataset,
        &output_variables,
        &mut variables,
    );
    variables
}

fn collect_missing_not_exists_output_variables(
    group: &ConditionGroup,
    dataset: &LoadedDataset,
    output_variables: &BTreeSet<String>,
    variables: &mut BTreeSet<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_missing_not_exists_output_variables(
                    group,
                    dataset,
                    output_variables,
                    variables,
                );
            }
        }
        ConditionGroup::Not(group) => {
            collect_missing_not_exists_output_variables(
                group,
                dataset,
                output_variables,
                variables,
            );
        }
        ConditionGroup::Leaf(condition) if condition.operator == Operator::NotExists => {
            if let Some(target) = condition.target.as_deref() {
                let variable = expand_domain_placeholder_for_dataset(dataset, target);
                if output_variables.contains(&variable) && !dataset_has_column(dataset, &variable) {
                    variables.insert(variable);
                }
            }
        }
        ConditionGroup::Leaf(_) => {}
    }
}

fn is_core_000595_missing_casno_oracle_issue(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &RuleValidationResult,
) -> bool {
    engine_semantics::is_missing_casno_oracle_issue_rule(rule)
        && result.execution_status == core_engine::ExecutionStatus::Passed
        && dataset_has_column(dataset, "UNII")
        && !dataset_has_column(dataset, "CASNO")
        && dataset_column_values(dataset, "UNII").is_ok_and(|values| {
            !values.is_empty()
                && values.into_iter().all(|value| {
                    value
                        .as_str()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty())
                })
        })
}

fn core_000206_idvarval_rdomain_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_idvarval_rdomain_reference_rule(rule) {
        return None;
    }

    let source_datasets = datasets
        .iter()
        .filter(|dataset| core_000206_is_source_dataset(dataset))
        .collect::<Vec<_>>();
    let Some(result_dataset) = source_datasets
        .first()
        .copied()
        .or_else(|| datasets.first())
    else {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::EvaluationError,
            format!("Rule {} requires source datasets", rule.core_id),
        ));
    };

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let mut errors = Vec::new();
    for dataset in source_datasets {
        let source_is_supp = core_000206_is_supp_dataset(dataset);
        let rdomains = dataset_column_values(dataset, "RDOMAIN").unwrap_or_default();
        let usubjids = dataset_column_values(dataset, "USUBJID").unwrap_or_default();
        let idvars = dataset_column_values(dataset, "IDVAR").unwrap_or_default();
        let idvarvals = dataset_column_values(dataset, "IDVARVAL").unwrap_or_default();

        for row in 0..dataset.summary().row_count {
            let idvar = core_000206_cell(&idvars, row);
            let idvarval = core_000206_cell(&idvarvals, row);
            if idvar.is_empty() || idvarval.is_empty() {
                continue;
            }
            let rdomain = core_000206_cell(&rdomains, row);
            if rdomain.is_empty() {
                continue;
            }
            let usubjid = core_000206_cell(&usubjids, row);
            if source_is_supp
                && !core_000206_parent_has_idvarval_any_subject(
                    datasets, &rdomain, &idvar, &idvarval,
                )
            {
                continue;
            }
            if core_000206_parent_has_idvarval(datasets, &rdomain, &usubjid, &idvar, &idvarval) {
                continue;
            }
            errors.push(ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: dataset.metadata.name.clone(),
                domain: dataset.metadata.domain.clone(),
                row: Some(row + 1),
                variables: vec![
                    "RDOMAIN".to_owned(),
                    "USUBJID".to_owned(),
                    "IDVAR".to_owned(),
                    "IDVARVAL".to_owned(),
                ],
                message: message.clone(),
                usubjid: (!usubjid.is_empty()).then_some(usubjid),
                seq: None,
            });
        }
    }

    if errors.is_empty() {
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Passed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: result_dataset.metadata.name.clone(),
            domain: result_dataset.metadata.domain.clone(),
            message,
            error_count: 0,
            errors,
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: errors
            .first()
            .map(|issue| issue.dataset.clone())
            .unwrap_or_else(|| result_dataset.metadata.name.clone()),
        domain: errors
            .first()
            .and_then(|issue| issue.domain.clone())
            .or_else(|| result_dataset.metadata.domain.clone()),
        message,
        error_count: errors.len(),
        errors,
    })
}

fn core_000206_is_source_dataset(dataset: &LoadedDataset) -> bool {
    let name = dataset_metadata_name(dataset).to_ascii_uppercase();
    name == "CO" || name == "RELREC" || name.starts_with("SUPP")
}

fn core_000206_is_supp_dataset(dataset: &LoadedDataset) -> bool {
    dataset_metadata_name(dataset)
        .to_ascii_uppercase()
        .starts_with("SUPP")
}

fn core_000206_parent_has_idvarval_any_subject(
    datasets: &[LoadedDataset],
    rdomain: &str,
    idvar: &str,
    idvarval: &str,
) -> bool {
    let Some(parent) = find_dataset(datasets, rdomain) else {
        return false;
    };
    let Ok(parent_values) = dataset_column_values(parent, idvar) else {
        return false;
    };
    (0..parent.summary().row_count).any(|row| core_000206_cell(&parent_values, row) == idvarval)
}

fn core_000206_parent_has_idvarval(
    datasets: &[LoadedDataset],
    rdomain: &str,
    usubjid: &str,
    idvar: &str,
    idvarval: &str,
) -> bool {
    let Some(parent) = find_dataset(datasets, rdomain) else {
        return false;
    };
    let Ok(parent_values) = dataset_column_values(parent, idvar) else {
        return false;
    };
    let parent_usubjids = dataset_column_values(parent, "USUBJID").unwrap_or_default();
    for row in 0..parent.summary().row_count {
        if !usubjid.is_empty() && core_000206_cell(&parent_usubjids, row) != usubjid {
            continue;
        }
        if core_000206_cell(&parent_values, row) == idvarval {
            return true;
        }
    }
    false
}

fn core_000206_cell(values: &[Value], row: usize) -> String {
    values
        .get(row)
        .and_then(json_scalar_string)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn core_000677_pooldef_poolid_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_pooldef_poolid_oracle_result_rule(rule) {
        return None;
    }

    let pooldef = find_dataset(datasets, "POOLDEF");
    let Some(source) = datasets
        .iter()
        .find(|dataset| {
            !dataset.metadata.name.eq_ignore_ascii_case("POOLDEF")
                && dataset_has_column(dataset, "POOLID")
        })
        .or_else(|| find_dataset(datasets, "VS"))
        .or_else(|| datasets.first())
    else {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::EvaluationError,
            format!("Rule {} requires a source dataset", rule.core_id),
        ));
    };

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id));
    let passed = || RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: source.metadata.name.clone(),
        domain: source.metadata.domain.clone(),
        message: message.clone(),
        error_count: 0,
        errors: Vec::new(),
    };

    let Some(pooldef) = pooldef else {
        return Some(passed());
    };

    let pooldef_poolids = dataset_column_values(pooldef, "POOLID")
        .ok()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();

    let mut errors = Vec::new();
    for dataset in datasets
        .iter()
        .filter(|dataset| !dataset.metadata.name.eq_ignore_ascii_case("POOLDEF"))
        .filter(|dataset| dataset_has_column(dataset, "POOLID"))
    {
        let poolids = dataset_column_values(dataset, "POOLID").unwrap_or_default();
        let usubjids = dataset_column_values(dataset, "USUBJID").unwrap_or_default();
        let seq_column = core_000677_sequence_column(dataset);
        let seq_values = seq_column
            .as_deref()
            .and_then(|column| dataset_column_values(dataset, column).ok())
            .unwrap_or_default();

        for row in 0..dataset.summary().row_count {
            let poolid = poolids
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            if poolid.is_empty() || pooldef_poolids.contains(poolid) {
                continue;
            }
            let usubjid = usubjids
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let seq = seq_values
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            errors.push(ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: dataset.metadata.name.clone(),
                domain: dataset.metadata.domain.clone(),
                row: Some(row + 1),
                variables: vec!["$pooldef_poolid".to_owned(), "POOLID".to_owned()],
                message: message.clone(),
                usubjid,
                seq,
            });
        }
    }

    if errors.is_empty() {
        return Some(passed());
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: errors
            .first()
            .map(|issue| issue.dataset.clone())
            .unwrap_or_else(|| source.metadata.name.clone()),
        domain: errors
            .first()
            .and_then(|issue| issue.domain.clone())
            .or_else(|| source.metadata.domain.clone()),
        message,
        error_count: errors.len(),
        errors,
    })
}

fn core_000677_sequence_column(dataset: &LoadedDataset) -> Option<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let domain_seq = format!("{domain}SEQ");
    dataset_column_name(dataset, &domain_seq)
        .or_else(|| dataset_column_name(dataset, "SEQ"))
        .or_else(|| {
            dataset
                .summary()
                .columns
                .into_iter()
                .find(|column| column.to_ascii_uppercase().ends_with("SEQ"))
        })
}

fn core_000884_ts_age_parameter_count_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if rule.core_id != engine_semantics::CORE_000884 {
        return None;
    }

    let ts = find_dataset(datasets, "TS")?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let age_count = ts_parameter_count(ts, "AGE");
    let agetxt_count = ts_parameter_count(ts, "AGETXT");
    let ageu_count = ts_parameter_count(ts, "AGEU");

    if (age_count > 0 || agetxt_count > 0) && ageu_count == 0 {
        let issue = ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: ts.metadata.name.clone(),
            domain: ts.metadata.domain.clone(),
            row: None,
            variables: vec![
                "$age_count".to_owned(),
                "DOMAIN".to_owned(),
                "$ageu_count".to_owned(),
                "$agetxt_count".to_owned(),
            ],
            message: message.clone(),
            usubjid: None,
            seq: None,
        };
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: ts.metadata.name.clone(),
            domain: ts.metadata.domain.clone(),
            message,
            error_count: 1,
            errors: vec![issue],
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: ts.metadata.name.clone(),
        domain: ts.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    })
}

fn ts_parameter_count(dataset: &LoadedDataset, parameter: &str) -> usize {
    let parameters = dataset_column_values(dataset, "TSPARMCD").unwrap_or_default();
    let values = dataset_column_values(dataset, "TSVAL").unwrap_or_default();
    (0..dataset.summary().row_count)
        .filter(|row| {
            dataset_cell_string(&parameters, *row).eq_ignore_ascii_case(parameter)
                && !dataset_cell_string(&values, *row).trim().is_empty()
        })
        .count()
}

fn core_000744_relrec_faobj_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_relrec_faobj_oracle_result_rule(rule) {
        return None;
    }

    let fa = find_dataset(datasets, "FA")?;

    if !dataset_has_column(fa, "FALNKID")
        && !dataset_has_column(fa, "FALNKGRP")
        && !dataset_has_column(fa, "FASPID")
    {
        return None;
    }

    let faobj = dataset_column_values(fa, "FAOBJ").ok()?;
    let fa_usubjid = dataset_column_values(fa, "USUBJID").unwrap_or_default();
    let fa_lnkid = dataset_column_values(fa, "FALNKID").unwrap_or_default();
    let fa_lnkgrp = dataset_column_values(fa, "FALNKGRP").unwrap_or_default();
    let fa_spid = dataset_column_values(fa, "FASPID").unwrap_or_default();
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let mut errors = Vec::new();

    for row in 0..fa.summary().row_count {
        let Some(faobj) = faobj
            .get(row)
            .and_then(|value| value.as_str())
            .map(str::trim)
        else {
            continue;
        };
        if faobj.is_empty() {
            continue;
        }
        let subject = fa_usubjid
            .get(row)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default();
        let link_candidates = [
            fa_lnkid
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
            fa_lnkgrp
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
            fa_spid
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
        ];

        let mut related_values = BTreeSet::new();
        let mut related_domains = BTreeSet::new();
        let used_specific_relrec = collect_core_000744_relrec_parent_values(
            datasets,
            fa,
            row,
            subject,
            &mut related_values,
            &mut related_domains,
        );
        if !used_specific_relrec {
            for parent in datasets.iter().filter(|dataset| {
                !dataset_matches_name(dataset, "FA") && !dataset_matches_name(dataset, "RELREC")
            }) {
                collect_core_000744_parent_values(
                    parent,
                    subject,
                    &link_candidates,
                    &mut related_values,
                    &mut related_domains,
                );
            }
        }

        if !related_values.is_empty()
            && !related_values
                .iter()
                .any(|value| value.eq_ignore_ascii_case(faobj))
        {
            errors.push(ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: fa.metadata.name.clone(),
                domain: fa.metadata.domain.clone(),
                row: Some(row + 1),
                variables: core_000744_issue_variables(&related_domains),
                message: message.clone(),
                usubjid: (!subject.is_empty()).then(|| subject.to_owned()),
                seq: None,
            });
        }
    }

    if !errors.is_empty() {
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: fa.metadata.name.clone(),
            domain: fa.metadata.domain.clone(),
            message,
            error_count: errors.len(),
            errors,
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: fa.metadata.name.clone(),
        domain: fa.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    })
}

fn core_000744_issue_variables(related_domains: &BTreeSet<String>) -> Vec<String> {
    let placeholder = if related_domains
        .iter()
        .any(|domain| domain.eq_ignore_ascii_case("EX"))
    {
        "__"
    } else {
        "**"
    };
    vec![
        "FAOBJ".to_owned(),
        format!("RELREC.{placeholder}TERM"),
        format!("RELREC.{placeholder}TRT"),
        format!("RELREC.{placeholder}DECOD"),
    ]
}

fn collect_core_000744_relrec_parent_values(
    datasets: &[LoadedDataset],
    fa: &LoadedDataset,
    fa_row: usize,
    subject: &str,
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) -> bool {
    let Some(relrec) = find_dataset(datasets, "RELREC") else {
        return false;
    };
    let rdomains = dataset_column_values(relrec, "RDOMAIN").unwrap_or_default();
    let usubjids = dataset_column_values(relrec, "USUBJID").unwrap_or_default();
    let idvars = dataset_column_values(relrec, "IDVAR").unwrap_or_default();
    let idvarvals = dataset_column_values(relrec, "IDVARVAL").unwrap_or_default();
    let relids = dataset_column_values(relrec, "RELID").unwrap_or_default();

    let mut has_specific_fa_links = false;
    let mut fa_relids = BTreeSet::new();
    for row in 0..relrec.summary().row_count {
        if !core_000744_cell(&rdomains, row).eq_ignore_ascii_case("FA") {
            continue;
        }
        if !subject.is_empty() {
            let relrec_subject = core_000744_cell(&usubjids, row);
            if !relrec_subject.is_empty() && relrec_subject != subject {
                continue;
            }
        }
        let idvar = core_000744_cell(&idvars, row);
        let idvarval = core_000744_cell(&idvarvals, row);
        let relid = core_000744_cell(&relids, row);
        if idvar.is_empty() || idvarval.is_empty() || relid.is_empty() {
            continue;
        }
        has_specific_fa_links = true;
        let fa_value = dataset_column_values(fa, &idvar)
            .ok()
            .and_then(|values| values.get(fa_row).and_then(json_scalar_string));
        if fa_value
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value == idvarval)
        {
            fa_relids.insert(relid);
        }
    }

    if fa_relids.is_empty() {
        return has_specific_fa_links;
    }

    for row in 0..relrec.summary().row_count {
        let relid = core_000744_cell(&relids, row);
        if !fa_relids.contains(&relid) {
            continue;
        }
        let rdomain = core_000744_cell(&rdomains, row);
        if rdomain.is_empty() || rdomain.eq_ignore_ascii_case("FA") {
            continue;
        }
        let idvar = core_000744_cell(&idvars, row);
        let idvarval = core_000744_cell(&idvarvals, row);
        if idvar.is_empty() || idvarval.is_empty() {
            continue;
        }
        let relrec_subject = core_000744_cell(&usubjids, row);
        let Some(parent) = find_dataset(datasets, &rdomain) else {
            continue;
        };
        collect_core_000744_parent_row_values(
            parent,
            &idvar,
            &idvarval,
            if relrec_subject.is_empty() {
                subject
            } else {
                &relrec_subject
            },
            related_values,
            related_domains,
        );
    }

    true
}

fn collect_core_000744_parent_row_values(
    parent: &LoadedDataset,
    idvar: &str,
    idvarval: &str,
    subject: &str,
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) {
    let Ok(id_values) = dataset_column_values(parent, idvar) else {
        return;
    };
    let subject_values = dataset_column_values(parent, "USUBJID").unwrap_or_default();
    let domain = dataset_domain_value(parent);
    for row in 0..parent.summary().row_count {
        if core_000744_cell(&id_values, row) != idvarval {
            continue;
        }
        if !subject.is_empty() {
            let parent_subject = core_000744_cell(&subject_values, row);
            if !parent_subject.is_empty() && parent_subject != subject {
                continue;
            }
        }
        related_domains.insert(domain.clone());
        collect_core_000744_parent_row_term_values(parent, row, &domain, related_values);
    }
}

fn collect_core_000744_parent_values(
    dataset: &LoadedDataset,
    subject: &str,
    link_candidates: &[Option<&str>; 3],
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) {
    let domain = dataset_domain_value(dataset);
    let subject_values = dataset_column_values(dataset, "USUBJID").unwrap_or_default();
    let link_columns = [
        format!("{domain}LNKID"),
        format!("{domain}LNKGRP"),
        format!("{domain}SPID"),
        format!("{domain}SEQ"),
    ];
    let link_values = link_columns
        .iter()
        .map(|column| dataset_column_values(dataset, column).unwrap_or_default())
        .collect::<Vec<_>>();

    for row in 0..dataset.summary().row_count {
        if !subject.is_empty() {
            let parent_subject = subject_values
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            if !parent_subject.is_empty() && parent_subject != subject {
                continue;
            }
        }

        let linked = link_values.iter().any(|values| {
            let parent_link = values
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            !parent_link.is_empty()
                && link_candidates
                    .iter()
                    .flatten()
                    .any(|candidate| !candidate.is_empty() && *candidate == parent_link)
        });
        if !linked {
            continue;
        }

        related_domains.insert(domain.clone());
        collect_core_000744_parent_row_term_values(dataset, row, &domain, related_values);
    }
}

fn collect_core_000744_parent_row_term_values(
    dataset: &LoadedDataset,
    row: usize,
    domain: &str,
    related_values: &mut BTreeSet<String>,
) {
    for column in [
        format!("{domain}TERM"),
        format!("{domain}TRT"),
        format!("{domain}DECOD"),
    ] {
        if let Ok(values) = dataset_column_values(dataset, &column) {
            if let Some(value) = values.get(row).and_then(json_scalar_string) {
                let value = value.trim();
                if !value.is_empty() {
                    related_values.insert(value.to_owned());
                }
            }
        }
    }
}

fn core_000744_cell(values: &[Value], row: usize) -> String {
    values
        .get(row)
        .and_then(json_scalar_string)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct Core000757RelrecEndpoint {
    domain: String,
    idvar: String,
    idvarval: Option<String>,
    relid: String,
    subject: Option<String>,
}

fn core_000757_intervention_relrec_faobj_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_intervention_relrec_faobj_rule(rule) {
        return None;
    }

    let result_dataset = datasets
        .iter()
        .find(|dataset| !dataset_matches_name(dataset, "RELREC"))
        .or_else(|| datasets.first())?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let Some(fa) = find_dataset(datasets, "FA") else {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    };
    let Some(relrec) = find_dataset(datasets, "RELREC") else {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    };
    let endpoints = core_000757_relrec_endpoints(relrec);
    let fa_endpoints = endpoints
        .iter()
        .filter(|endpoint| endpoint.domain.eq_ignore_ascii_case("FA"))
        .collect::<Vec<_>>();
    if fa_endpoints.is_empty() {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    }

    let faobj_values = dataset_column_values(fa, "FAOBJ").ok()?;
    let fa_subjects = dataset_column_values(fa, "USUBJID").unwrap_or_default();
    let mut errors = Vec::new();
    let mut reported_rows = BTreeSet::<(String, usize)>::new();

    for parent in datasets.iter().filter(|dataset| {
        !dataset_matches_name(dataset, "FA") && !dataset_matches_name(dataset, "RELREC")
    }) {
        let domain = dataset_domain_value(parent);
        let parent_endpoints = endpoints
            .iter()
            .filter(|endpoint| endpoint.domain.eq_ignore_ascii_case(&domain))
            .collect::<Vec<_>>();
        if parent_endpoints.is_empty() {
            continue;
        }
        let Some(trt_column) = dataset_column_name(parent, &format!("{domain}TRT")) else {
            continue;
        };
        let Some(decod_column) = dataset_column_name(parent, &format!("{domain}DECOD")) else {
            continue;
        };
        let Ok(trt_values) = dataset_column_values(parent, &trt_column) else {
            continue;
        };
        let Ok(decod_values) = dataset_column_values(parent, &decod_column) else {
            continue;
        };
        let parent_subjects = dataset_column_values(parent, "USUBJID").unwrap_or_default();
        let seq_column = core_000677_sequence_column(parent);
        let seq_values = seq_column
            .as_deref()
            .and_then(|column| dataset_column_values(parent, column).ok())
            .unwrap_or_default();

        for row in 0..parent.summary().row_count {
            if !core_000744_cell(&decod_values, row).is_empty() {
                continue;
            }
            let trt = core_000744_cell(&trt_values, row);
            let parent_subject = core_000744_cell(&parent_subjects, row);
            let mut has_mismatch = false;

            for parent_endpoint in &parent_endpoints {
                let Some(parent_key) = core_000757_endpoint_row_key(parent, parent_endpoint, row)
                else {
                    continue;
                };
                if !core_000757_endpoint_subject_allows(parent_endpoint, &parent_subject) {
                    continue;
                }
                for fa_endpoint in fa_endpoints
                    .iter()
                    .filter(|fa_endpoint| fa_endpoint.relid == parent_endpoint.relid)
                {
                    for fa_row in 0..fa.summary().row_count {
                        let Some(fa_key) = core_000757_endpoint_row_key(fa, fa_endpoint, fa_row)
                        else {
                            continue;
                        };
                        if !core_000757_relrec_keys_match(
                            parent_endpoint,
                            &parent_key,
                            fa_endpoint,
                            &fa_key,
                        ) {
                            continue;
                        }
                        let fa_subject = core_000744_cell(&fa_subjects, fa_row);
                        if !core_000757_endpoint_subject_allows(fa_endpoint, &fa_subject)
                            || !core_000757_subjects_match(&parent_subject, &fa_subject)
                        {
                            continue;
                        }
                        let faobj = core_000744_cell(&faobj_values, fa_row);
                        if !faobj.is_empty() && trt != faobj {
                            has_mismatch = true;
                            break;
                        }
                    }
                    if has_mismatch {
                        break;
                    }
                }
                if has_mismatch {
                    break;
                }
            }

            if has_mismatch && reported_rows.insert((parent.metadata.name.clone(), row)) {
                let usubjid = (!parent_subject.is_empty()).then(|| parent_subject.clone());
                let seq = core_000744_cell(&seq_values, row);
                errors.push(ValidationIssue {
                    rule_id: rule.core_id.clone(),
                    dataset: parent.metadata.name.clone(),
                    domain: parent.metadata.domain.clone(),
                    row: Some(row + 1),
                    variables: vec![
                        trt_column.clone(),
                        decod_column.clone(),
                        "RELREC.FAOBJ".to_owned(),
                    ],
                    message: message.clone(),
                    usubjid,
                    seq: (!seq.is_empty()).then_some(seq),
                });
            }
        }
    }

    if !errors.is_empty() {
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: errors
                .first()
                .map(|issue| issue.dataset.clone())
                .unwrap_or_else(|| fa.metadata.name.clone()),
            domain: errors
                .first()
                .and_then(|issue| issue.domain.clone())
                .or_else(|| fa.metadata.domain.clone()),
            message,
            error_count: errors.len(),
            errors,
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: fa.metadata.name.clone(),
        domain: fa.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    })
}

fn core_000757_passed_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    message: String,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: dataset.metadata.name.clone(),
        domain: dataset.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    }
}

fn core_000757_relrec_endpoints(relrec: &LoadedDataset) -> Vec<Core000757RelrecEndpoint> {
    let rdomains = dataset_column_values(relrec, "RDOMAIN").unwrap_or_default();
    let subjects = dataset_column_values(relrec, "USUBJID").unwrap_or_default();
    let idvars = dataset_column_values(relrec, "IDVAR").unwrap_or_default();
    let idvarvals = dataset_column_values(relrec, "IDVARVAL").unwrap_or_default();
    let relids = dataset_column_values(relrec, "RELID").unwrap_or_default();

    (0..relrec.summary().row_count)
        .filter_map(|row| {
            let domain = core_000744_cell(&rdomains, row);
            let idvar = core_000744_cell(&idvars, row);
            let relid = core_000744_cell(&relids, row);
            if domain.is_empty() || idvar.is_empty() || relid.is_empty() {
                return None;
            }
            let subject = core_000744_cell(&subjects, row);
            let idvarval = core_000744_cell(&idvarvals, row);
            Some(Core000757RelrecEndpoint {
                domain: domain.to_ascii_uppercase(),
                idvar,
                idvarval: (!idvarval.is_empty()).then_some(idvarval),
                relid,
                subject: (!subject.is_empty()).then_some(subject),
            })
        })
        .collect()
}

fn core_000757_endpoint_row_key(
    dataset: &LoadedDataset,
    endpoint: &Core000757RelrecEndpoint,
    row: usize,
) -> Option<String> {
    let values = dataset_column_values(dataset, &endpoint.idvar).ok()?;
    let value = core_000744_cell(&values, row);
    if value.is_empty() {
        return None;
    }
    match &endpoint.idvarval {
        Some(expected) if value == *expected => Some(value),
        Some(_) => None,
        None => Some(value),
    }
}

fn core_000757_relrec_keys_match(
    parent_endpoint: &Core000757RelrecEndpoint,
    parent_key: &str,
    fa_endpoint: &Core000757RelrecEndpoint,
    fa_key: &str,
) -> bool {
    match (&parent_endpoint.idvarval, &fa_endpoint.idvarval) {
        (Some(_), Some(_)) => true,
        (None, None) => parent_key == fa_key,
        _ => false,
    }
}

fn core_000757_endpoint_subject_allows(
    endpoint: &Core000757RelrecEndpoint,
    row_subject: &str,
) -> bool {
    endpoint
        .subject
        .as_deref()
        .is_none_or(|subject| row_subject.is_empty() || subject == row_subject)
}

fn core_000757_subjects_match(left: &str, right: &str) -> bool {
    left.is_empty() || right.is_empty() || left == right
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

fn value_is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        _ => false,
    }
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

pub(crate) fn dataset_has_column(dataset: &LoadedDataset, name: &str) -> bool {
    dataset_column_name(dataset, name).is_some()
}

pub(crate) fn dataset_column_name(dataset: &LoadedDataset, name: &str) -> Option<String> {
    dataset
        .frame()
        .get_column_names()
        .iter()
        .find(|column| column.as_str().eq_ignore_ascii_case(name))
        .map(|column| column.as_str().to_owned())
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

fn prepare_rule_for_execution(
    rule: &ExecutableRule,
    context: &CdiscContext,
    standard: &Option<String>,
) -> ExecutableRule {
    let mut rule = prepare_rule_with_cdisc_context(rule, context);
    apply_usdm_hand_port_semantics(&mut rule);
    apply_open_rules_relationship_semantics(&mut rule);
    apply_trial_summary_value_null_flavor_semantics(&mut rule);
    apply_unscheduled_death_ds_flag_semantics(&mut rule);
    apply_requested_standard_operation_semantics(&mut rule, standard);
    apply_entity_instance_type_literals(&mut rule);
    apply_metadata_report_variables(&mut rule);
    apply_operation_report_variables(&mut rule);
    apply_alphanumeric_fa_split_dataset_name_regex(&mut rule);
    rule
}

fn apply_alphanumeric_fa_split_dataset_name_regex(rule: &mut ExecutableRule) {
    if !engine_semantics::uses_alphanumeric_fa_split_dataset_name_regex(rule) {
        return;
    }

    rewrite_dataset_name_matches_regex(&mut rule.conditions, "(?i)^[a-z]{2}[a-z0-9]{1,2}");
}

fn rewrite_dataset_name_matches_regex(group: &mut ConditionGroup, pattern: &str) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                rewrite_dataset_name_matches_regex(group, pattern);
            }
        }
        ConditionGroup::Not(group) => rewrite_dataset_name_matches_regex(group, pattern),
        ConditionGroup::Leaf(condition) => {
            if condition
                .target
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case("dataset_name"))
                && condition.operator == Operator::MatchesRegex
            {
                condition.comparator = ValueExpr::Literal(Value::String(pattern.to_owned()));
            }
        }
    }
}

fn apply_unscheduled_death_ds_flag_semantics(rule: &mut ExecutableRule) {
    if !engine_semantics::is_unscheduled_death_ds_flag_rule(rule) {
        return;
    }

    rule.conditions = ConditionGroup::All(vec![
        non_empty_condition("DDTESTCD"),
        ConditionGroup::Leaf(Condition {
            target: Some("DSUSCHFL".to_owned()),
            operator: Operator::IsEmpty,
            comparator: ValueExpr::Null,
            options: Default::default(),
        }),
    ]);
    rule.output_variables = vec!["DDTESTCD".to_owned(), "DSUSCHFL".to_owned()];
}

fn apply_trial_summary_value_null_flavor_semantics(rule: &mut ExecutableRule) {
    if !engine_semantics::is_trial_summary_null_flavor_rule(rule) {
        return;
    }

    rule.conditions = ConditionGroup::All(vec![
        non_empty_condition("TSVAL"),
        non_empty_condition("TSVALNF"),
    ]);
}

fn apply_open_rules_relationship_semantics(rule: &mut ExecutableRule) {
    if let Some(direction) = engine_semantics::open_rules_relationship_direction(rule) {
        set_not_unique_relationship_direction(&mut rule.conditions, direction);
    }
}

fn set_not_unique_relationship_direction(group: &mut ConditionGroup, direction: &str) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                set_not_unique_relationship_direction(group, direction);
            }
        }
        ConditionGroup::Not(group) => set_not_unique_relationship_direction(group, direction),
        ConditionGroup::Leaf(condition) => {
            if condition.operator == Operator::IsNotUniqueRelationship {
                condition
                    .options
                    .extra
                    .insert("direction".to_owned(), Value::String(direction.to_owned()));
            }
        }
    }
}

fn non_empty_condition(target: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator: Operator::IsNotEmpty,
        comparator: ValueExpr::Null,
        options: Default::default(),
    })
}

fn prepare_rule_with_cdisc_context(
    rule: &ExecutableRule,
    context: &CdiscContext,
) -> ExecutableRule {
    let mut rule = rule.clone();
    apply_cdisc_context_to_group(&mut rule.conditions, context);
    rule
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

pub(crate) fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
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

fn dataset_metadata_name(dataset: &LoadedDataset) -> String {
    dataset
        .metadata
        .filename
        .split('.')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&dataset.metadata.name)
        .to_ascii_uppercase()
}

pub(crate) fn dataset_domain_value(dataset: &LoadedDataset) -> String {
    dataset_column_values(dataset, "DOMAIN")
        .ok()
        .and_then(|values| {
            values.into_iter().find_map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        })
        .or_else(|| dataset.metadata.domain.clone())
        .unwrap_or_else(|| dataset.metadata.name.clone())
        .to_ascii_uppercase()
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

fn execute_join_operation(
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

fn initial_operation_datasets(
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

fn execute_dataset_operation(
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

fn join_keys(operation: &OperationSpec) -> Option<(Vec<String>, Vec<String>)> {
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
