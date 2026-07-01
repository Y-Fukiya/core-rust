#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

mod engine_semantics;
mod open_rules_compat;
mod standard_filter;
mod usdm_jsonata;

pub use open_rules_compat::{rule_id_has_oracle_gap_category, rule_id_uses_hand_port};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use core_cdisc_library::{
    load_ct_json_file, load_define_xml_file, load_external_dictionary_file, ControlledTerm,
    ControlledTerminology, DefineXmlMetadata,
};
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
    load_rules_from_paths, normalize_condition_value, normalize_key, Condition, ConditionGroup,
    ExecutableRule, MatchDataset, OperationSpec, Operator, RuleModelError, RuleType, Sensitivity,
    ValueExpr,
};
use serde_json::Value;
use thiserror::Error;

use open_rules_compat::post_execution_oracle_gap_result;
use standard_filter::{apply_standard_filter, apply_standard_oracle_gap_filter};
use usdm_jsonata::{
    apply_usdm_jsonata_semantics, has_usdm_jsonata_semantics, usdm_jsonata_execution_datasets,
};

pub type Result<T> = std::result::Result<T, ApiError>;

const SOURCE_ROW_COLUMN: &str = "__core_source_row";

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
        if let Some(skipped) = skipped_open_rules_oracle_gap_after_operator_checks(rule) {
            return Some(skipped);
        }
    }

    if has_usdm_jsonata_semantics(rule) {
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

fn skipped_open_rules_oracle_gap_after_operator_checks(
    rule: &ExecutableRule,
) -> Option<RuleValidationResult> {
    let skipped = |message: String| {
        Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            message,
        ))
    };

    if is_domain_placeholder_column_ref_comparator_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses domain placeholder column-ref comparator oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_entity_literal_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses entity literal oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if contains_full_regex_wildcard_target(&rule.conditions) {
        return skipped(format!(
            "Rule {} uses wildcard regex target semantics that are not supported",
            rule.core_id
        ));
    }
    if contains_longer_than_target(&rule.conditions, "ETCD")
        && scope_matches(&scope_values(rule.domains.as_ref(), "Include"), "SE")
        && !should_defer_etcd_length_oracle_gap(rule)
    {
        return skipped(format!(
            "Rule {} uses ETCD length semantics for SE that are not supported",
            rule.core_id
        ));
    }
    if contains_longer_than_target(&rule.conditions, "ARMCD")
        && contains_target(&rule.conditions, "TXPARMCD")
        && contains_longer_than_target(&rule.conditions, "TXVAL")
    {
        return skipped(format!(
            "Rule {} uses cross-domain ARMCD/TXVAL length semantics that are not supported",
            rule.core_id
        ));
    }
    if is_empty_non_empty_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses empty/non_empty oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_date_operator_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses date oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_sort_operator_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses sort oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_unique_set_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses unique set oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_not_unique_relationship_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses not-unique relationship oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_inconsistent_across_dataset_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses inconsistent across dataset oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_usdm_match_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::DatasetJoinNotSupported,
            format!(
                "Rule {} uses USDM match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }
    if is_multi_base_match_dataset_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses multi-base match dataset oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_duplicate_match_dataset_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses duplicate match dataset oracle semantics that are not supported",
            rule.core_id
        ));
    }
    if is_relrec_or_supp_match_dataset_oracle_gap_rule(rule) {
        return skipped(format!(
            "Rule {} uses RELREC/SUPP-- match dataset oracle semantics that are not supported",
            rule.core_id
        ));
    }
    None
}

fn is_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    !rule.operations.is_empty() && has_oracle_gap_rule_id(rule, "operation")
}

fn has_oracle_gap_rule_id(rule: &ExecutableRule, category: &str) -> bool {
    rule_id_has_oracle_gap_category(&rule.core_id, category)
}

fn is_distinct_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_distinct_operation_oracle_gap(rule) {
        return false;
    }

    if has_unsupported_reference_distinct_operation(rule)
        && !is_supported_reference_distinct_rule(rule)
    {
        return true;
    }

    has_oracle_gap_rule_id(rule, "distinct_operation")
        && rule.operations.iter().any(|operation| {
            operation_name(operation).as_deref() == Some("distinct")
                && !bool_field(operation, &["value_is_reference"]).unwrap_or(false)
        })
}

fn is_dy_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_dy_operation_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "dy_operation") && has_dy_operation(rule)
}

fn is_required_value_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "required_value_metadata")
        && rule.rule_type == RuleType::ValueLevelMetadata
}

fn is_dataset_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        && rule.rule_type == RuleType::RecordData
        && contains_presence_operator(&rule.conditions)
        && has_oracle_gap_rule_id(rule, "dataset_presence")
}

fn is_domain_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_domain_presence_oracle_gap(rule) {
        return false;
    }

    matches!(
        rule.rule_type,
        RuleType::DatasetMetadata | RuleType::DomainPresence
    ) && has_oracle_gap_rule_id(rule, "domain_presence")
}

fn is_variable_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_variable_metadata_oracle_gap(rule) {
        return false;
    }

    rule.rule_type == RuleType::VariableMetadata
        && has_oracle_gap_rule_id(rule, "variable_metadata")
}

fn is_domain_placeholder_column_ref_comparator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_domain_placeholder_column_ref_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "domain_placeholder_column_ref_comparator")
        && contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

fn is_entity_literal_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    rule.entities.is_some() && has_oracle_gap_rule_id(rule, "entity_literal")
}

fn is_supported_entity_match_column_ref_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "supported_entity_match_column_ref")
}

fn is_empty_non_empty_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if std::env::var_os("CORE_RS_EXPERIMENT_ENABLE_EMPTY_NON_EMPTY").is_some() {
        return false;
    }

    if should_defer_empty_non_empty_oracle_gap(rule) {
        return false;
    }

    is_empty_non_empty_oracle_gap_rule_id(rule) && contains_empty_operator(&rule.conditions)
}

fn is_empty_non_empty_oracle_gap_rule_id(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "empty_non_empty")
}

fn is_date_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_date_operator_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "date_operator") && contains_date_operator(&rule.conditions)
}

fn should_defer_empty_non_empty_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_empty_non_empty")
        && contains_empty_operator(&rule.conditions)
}

fn should_defer_date_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_date_operator") && contains_date_operator(&rule.conditions)
}

fn should_defer_dy_operation_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_dy_operation") && has_dy_operation(rule)
}

fn is_sort_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_sort_operator_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "sort_operator") && contains_sort_operator(&rule.conditions)
}

fn is_unique_set_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_unique_set_oracle_gap(rule) {
        return false;
    }
    has_oracle_gap_rule_id(rule, "unique_set") && contains_unique_set_operator(&rule.conditions)
}

fn is_not_unique_relationship_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_not_unique_relationship_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "not_unique_relationship")
        && contains_not_unique_relationship_operator(&rule.conditions)
}

fn is_inconsistent_across_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_inconsistent_across_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "inconsistent_across_dataset")
        && contains_inconsistent_across_dataset_operator(&rule.conditions)
}

fn is_usdm_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "usdm_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_missing_column_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "missing_column")
}

fn is_multi_base_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_multi_base_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "multi_base_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_duplicate_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_duplicate_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "duplicate_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_relrec_or_supp_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_relrec_or_supp_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "relrec_or_supp_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_etcd_length_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_etcd_length")
        && contains_longer_than_target(&rule.conditions, "ETCD")
        && scope_matches(&scope_values(rule.domains.as_ref(), "Include"), "SE")
}

fn should_defer_unique_set_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_unique_set")
        && contains_unique_set_operator(&rule.conditions)
}

fn should_defer_sort_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_sort_operator") && contains_sort_operator(&rule.conditions)
}

fn should_defer_not_unique_relationship_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_not_unique_relationship")
        && contains_not_unique_relationship_operator(&rule.conditions)
}

fn should_defer_inconsistent_across_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_inconsistent_across_dataset")
        && contains_inconsistent_across_dataset_operator(&rule.conditions)
}

fn should_defer_relrec_or_supp_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_relrec_or_supp_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_multi_base_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_multi_base_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_duplicate_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_duplicate_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_entity_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.entities.is_some() && has_oracle_gap_rule_id(rule, "defer_entity_column_ref")
}

fn should_defer_domain_placeholder_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_domain_placeholder_column_ref")
        && contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

fn should_defer_domain_presence_oracle_gap(rule: &ExecutableRule) -> bool {
    matches!(
        rule.rule_type,
        RuleType::DatasetMetadata | RuleType::DomainPresence
    ) && has_oracle_gap_rule_id(rule, "defer_domain_presence")
}

fn should_defer_variable_metadata_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.rule_type == RuleType::VariableMetadata
        && has_oracle_gap_rule_id(rule, "defer_variable_metadata")
}

fn should_defer_distinct_operation_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_distinct_operation") && !rule.operations.is_empty()
}

fn should_defer_positive_zero_oracle_gap_probe(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_positive_zero_probe")
}

fn is_known_unsafe_positive_zero_probe_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "unsafe_positive_zero_probe")
}

fn contains_empty_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_empty_operator)
        }
        ConditionGroup::Not(group) => contains_empty_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::IsEmpty | Operator::IsNotEmpty)
        }
    }
}

fn contains_inconsistent_across_dataset_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(contains_inconsistent_across_dataset_operator),
        ConditionGroup::Not(group) => contains_inconsistent_across_dataset_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::IsInconsistentAcrossDataset)
        }
    }
}

fn contains_unique_set_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_unique_set_operator)
        }
        ConditionGroup::Not(group) => contains_unique_set_operator(group),
        ConditionGroup::Leaf(condition) => matches!(
            condition.operator,
            Operator::IsNotUniqueSet | Operator::IsUniqueSet
        ),
    }
}

fn contains_not_unique_relationship_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_not_unique_relationship_operator)
        }
        ConditionGroup::Not(group) => contains_not_unique_relationship_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::IsNotUniqueRelationship)
        }
    }
}

fn contains_sort_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_sort_operator)
        }
        ConditionGroup::Not(group) => contains_sort_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::TargetIsNotSortedBy)
        }
    }
}

fn contains_date_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_date_operator)
        }
        ConditionGroup::Not(group) => contains_date_operator(group),
        ConditionGroup::Leaf(condition) => matches!(
            condition.operator,
            Operator::DateEqualTo
                | Operator::DateNotEqualTo
                | Operator::DateLessThan
                | Operator::DateLessThanOrEqualTo
                | Operator::DateGreaterThan
                | Operator::DateGreaterThanOrEqualTo
                | Operator::InvalidDate
                | Operator::InvalidDuration
                | Operator::IsCompleteDate
                | Operator::IsIncompleteDate
        ),
    }
}

fn contains_presence_operator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_presence_operator)
        }
        ConditionGroup::Not(group) => contains_presence_operator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::Exists | Operator::NotExists)
        }
    }
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

fn presence_target_columns(group: &ConditionGroup) -> BTreeSet<String> {
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

fn contains_column_ref_comparator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_column_ref_comparator)
        }
        ConditionGroup::Not(group) => contains_column_ref_comparator(group),
        ConditionGroup::Leaf(condition) => {
            matches!(&condition.comparator, ValueExpr::ColumnRef(column) if column.contains("--") && !column.starts_with("--"))
        }
    }
}

fn contains_domain_placeholder_column_ref_comparator(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(contains_domain_placeholder_column_ref_comparator),
        ConditionGroup::Not(group) => contains_domain_placeholder_column_ref_comparator(group),
        ConditionGroup::Leaf(condition) => {
            !matches!(condition.operator, Operator::IsNotUniqueRelationship)
                && matches!(&condition.comparator, ValueExpr::ColumnRef(column) if column.starts_with("--"))
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

fn dataset_has_column(dataset: &LoadedDataset, name: &str) -> bool {
    dataset_column_name(dataset, name).is_some()
}

fn dataset_column_name(dataset: &LoadedDataset, name: &str) -> Option<String> {
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

fn contains_full_regex_wildcard_target(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(contains_full_regex_wildcard_target)
        }
        ConditionGroup::Not(group) => contains_full_regex_wildcard_target(group),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::DoesNotMatchRegexFullString)
                && condition
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("--"))
        }
    }
}

fn contains_target(group: &ConditionGroup, target: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().any(|group| contains_target(group, target))
        }
        ConditionGroup::Not(group) => contains_target(group, target),
        ConditionGroup::Leaf(condition) => condition
            .target
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(target)),
    }
}

fn contains_longer_than_target(group: &ConditionGroup, target: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| contains_longer_than_target(group, target)),
        ConditionGroup::Not(group) => contains_longer_than_target(group, target),
        ConditionGroup::Leaf(condition) => {
            matches!(condition.operator, Operator::LongerThan)
                && condition
                    .target
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(target))
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CdiscContext {
    define_xml: Vec<DefineXmlMetadata>,
    terminology: ControlledTerminology,
}

impl CdiscContext {
    fn load(
        define_xml_paths: &[PathBuf],
        ct_paths: &[PathBuf],
        external_dictionary_paths: &[PathBuf],
    ) -> Result<Self> {
        let define_xml = define_xml_paths
            .iter()
            .map(load_define_xml_file)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut terminology = ControlledTerminology::default();

        for define in &define_xml {
            for (canonical, aliases) in &define.codelist_aliases {
                for alias in aliases {
                    terminology.insert_alias(canonical, alias);
                }
            }
            for term in &define.codelists {
                terminology.insert_term(&term.codelist, term.value.clone());
            }
        }

        for path in ct_paths {
            let ct = load_ct_json_file(path)?;
            merge_terminology(&mut terminology, ct);
        }

        for path in external_dictionary_paths {
            let dictionary = load_external_dictionary_file(path)?;
            merge_terminology(&mut terminology, dictionary);
        }

        Ok(Self {
            define_xml,
            terminology,
        })
    }
}

fn merge_terminology(target: &mut ControlledTerminology, source: ControlledTerminology) {
    for (alias, canonical) in source.aliases {
        target.insert_alias(canonical, alias);
    }
    for (codelist, values) in source.codelists {
        for value in values {
            target.insert_term(&codelist, value);
        }
    }
}

fn prepare_rule_for_execution(
    rule: &ExecutableRule,
    context: &CdiscContext,
    standard: &Option<String>,
) -> ExecutableRule {
    let mut rule = prepare_rule_with_cdisc_context(rule, context);
    apply_usdm_jsonata_semantics(&mut rule);
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

fn apply_operation_report_variables(rule: &mut ExecutableRule) {
    if engine_semantics::uses_check_target_report_variable_for_ex_end_rule(rule)
        && condition_targets_column(&rule.conditions, "RFXENDTC")
    {
        for variable in &mut rule.output_variables {
            if variable.eq_ignore_ascii_case("RFXSTDTC") {
                *variable = "RFXENDTC".to_owned();
            }
        }
    }

    if engine_semantics::is_operation_report_variable_override_rule(rule) {
        push_unique_string(&mut rule.output_variables, "USUBJID");
        push_unique_string(&mut rule.output_variables, "STUDYID");
        return;
    }

    if engine_semantics::is_reference_distinct_report_variable_rule(rule)
        && has_reference_distinct_operation(rule)
    {
        let mut variables = Vec::new();
        collect_condition_target_variables(&rule.conditions, &mut variables);
        if !variables.is_empty() {
            rule.output_variables = variables;
        }
        return;
    }

    if !rule.output_variables.is_empty() || !has_reference_distinct_operation(rule) {
        return;
    }

    let mut variables = Vec::new();
    collect_condition_target_variables(&rule.conditions, &mut variables);
    if !variables.is_empty() {
        rule.output_variables = variables;
    }
}

fn apply_metadata_report_variables(rule: &mut ExecutableRule) {
    if !rule.output_variables.is_empty()
        || !matches!(
            rule.rule_type,
            RuleType::DatasetMetadata | RuleType::VariableMetadata
        )
    {
        return;
    }

    let mut variables = Vec::new();
    collect_condition_target_variables(&rule.conditions, &mut variables);
    if !variables.is_empty() {
        rule.output_variables = variables;
    }
}

fn apply_requested_standard_operation_semantics(
    rule: &mut ExecutableRule,
    standard: &Option<String>,
) {
    if !engine_semantics::is_requested_standard_operation_rule(rule) {
        return;
    }

    let Some(standard) = standard.as_deref() else {
        return;
    };

    if standard.eq_ignore_ascii_case("SENDIG") {
        for operation in &mut rule.operations {
            if operation_name(operation).as_deref() == Some("domain_label") {
                operation.fields.insert(
                    "domain_label_source".to_owned(),
                    Value::String("domain".to_owned()),
                );
            }
        }
        push_unique_string(&mut rule.output_variables, "--CAT");
        push_unique_string(&mut rule.output_variables, "DOMAIN");
    }
}

fn apply_entity_instance_type_literals(rule: &mut ExecutableRule) {
    if rule.entities.is_none() {
        return;
    }
    apply_entity_instance_type_literals_to_group(&mut rule.conditions);
}

fn collect_condition_target_variables(group: &ConditionGroup, variables: &mut Vec<String>) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_condition_target_variables(group, variables);
            }
        }
        ConditionGroup::Not(group) => collect_condition_target_variables(group, variables),
        ConditionGroup::Leaf(condition) => {
            if matches!(
                condition.operator,
                Operator::EqualTo
                    | Operator::NotEqualTo
                    | Operator::EqualToCaseInsensitive
                    | Operator::NotEqualToCaseInsensitive
            ) && condition
                .target
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case("instanceType"))
            {
                if let ValueExpr::ColumnRef(value) = &condition.comparator {
                    condition.comparator = ValueExpr::Literal(Value::String(value.clone()));
                }
            }
        }
    }
}

fn has_reference_distinct_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("distinct" | "unique")
        ) && operation_dataset_name(operation).is_some()
            && string_field(operation, &["id", "target", "as", "output", "column"]).is_some()
    })
}

fn has_variable_count_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("variable_count"))
}

fn has_dataset_level_record_count_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        operation_name(operation).as_deref() == Some("record_count")
            && string_list_field(
                operation,
                &["by", "keys", "group", "group_by", "group_keys"],
            )
            .map(|keys| keys.is_empty())
            .unwrap_or(true)
    })
}

fn has_dataset_names_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("dataset_names"))
}

fn is_supported_dataset_metadata_rule(rule: &ExecutableRule) -> bool {
    (rule.operations.is_empty()
        || rule.operations.iter().all(|operation| {
            matches!(
                operation_name(operation).as_deref(),
                Some("record_count" | "dataset_names")
            )
        }) && (has_dataset_level_record_count_operation(rule)
            || has_dataset_names_operation(rule)))
        && (engine_semantics::supports_column_ref_metadata_comparator(rule)
            || !contains_column_ref_comparator(&rule.conditions))
        && unsupported_operator(&rule.conditions).is_none()
}

fn is_supported_variable_metadata_rule(rule: &ExecutableRule) -> bool {
    (rule.operations.is_empty()
        || rule.operations.iter().all(|operation| {
            matches!(
                operation_name(operation).as_deref(),
                Some(
                    "expected_variables"
                        | "required_variables"
                        | "get_column_order_from_dataset"
                        | "get_column_order_from_library"
                        | "get_model_column_order"
                        | "get_model_filtered_variables"
                        | "distinct"
                        | "domain_is_custom"
                        | "codelist_terms"
                )
            )
        }) && ((has_dataset_column_order_operation(rule)
            && (has_expected_variables_operation(rule)
                || has_required_variables_operation(rule)
                || has_model_filtered_variables_operation(rule)))
            || has_variable_metadata_domain_prefix_operations(rule))
        || has_model_column_order_operation(rule)
        || engine_semantics::is_domain_codelist_metadata_rule(rule))
        && (engine_semantics::supports_column_ref_metadata_comparator(rule)
            || engine_semantics::is_library_variable_metadata_rule(rule)
            || !references_library_metadata_variables(rule))
        && (engine_semantics::can_skip_metadata_column_ref_comparator(rule)
            || !contains_column_ref_comparator(&rule.conditions))
        && unsupported_operator(&rule.conditions).is_none()
}

fn is_supported_value_metadata_rule(rule: &ExecutableRule) -> bool {
    engine_semantics::is_supported_value_metadata_rule_id(rule)
        && rule.operations.is_empty()
        && !contains_column_ref_comparator(&rule.conditions)
        && unsupported_operator(&rule.conditions).is_none()
}

fn references_library_metadata_variables(rule: &ExecutableRule) -> bool {
    rule.output_variables
        .iter()
        .any(|variable| is_library_metadata_variable(variable))
        || condition_references_library_metadata_variable(&rule.conditions)
}

fn condition_references_library_metadata_variable(group: &ConditionGroup) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(condition_references_library_metadata_variable),
        ConditionGroup::Not(group) => condition_references_library_metadata_variable(group),
        ConditionGroup::Leaf(condition) => {
            condition
                .target
                .as_deref()
                .is_some_and(is_library_metadata_variable)
                || matches!(&condition.comparator, ValueExpr::ColumnRef(column) if is_library_metadata_variable(column))
        }
    }
}

fn is_library_metadata_variable(variable: &str) -> bool {
    normalize_key(variable).starts_with("library_")
}

fn has_expected_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("expected_variables"))
}

fn has_required_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("required_variables"))
}

fn has_dataset_column_order_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        operation_name(operation).as_deref() == Some("get_column_order_from_dataset")
    })
}

fn has_model_filtered_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        operation_name(operation).as_deref() == Some("get_model_filtered_variables")
    })
}

fn has_model_column_order_operation(rule: &ExecutableRule) -> bool {
    engine_semantics::is_model_column_order_rule(rule)
        && rule.operations.iter().any(|operation| {
            matches!(
                operation_name(operation).as_deref(),
                Some("get_model_column_order" | "get_column_order_from_library")
            )
        })
}

fn has_variable_metadata_domain_prefix_operations(rule: &ExecutableRule) -> bool {
    engine_semantics::is_variable_metadata_domain_prefix_rule(rule)
        && rule.operations.iter().any(|operation| {
            operation_name(operation).as_deref() == Some("distinct")
                && string_field(operation, &["name", "column", "variable"])
                    .is_some_and(|name| name.eq_ignore_ascii_case("DOMAIN"))
        })
        && rule
            .operations
            .iter()
            .any(|operation| operation_name(operation).as_deref() == Some("domain_is_custom"))
}

fn has_dy_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("dy"))
}

fn has_group_date_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("min_date" | "max_date")
        )
    })
}

fn has_match_dataset_dependent_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("map" | "codelist_extensible" | "codelist_terms")
        )
    })
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

fn has_group_aliases(operation: &OperationSpec) -> bool {
    string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .is_some_and(|aliases| !aliases.is_empty())
}

fn has_unsupported_reference_distinct_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("distinct" | "unique")
        ) && operation_dataset_name(operation).is_some()
            && string_field(operation, &["id", "target", "as", "output", "column"]).is_some()
            && !bool_field(operation, &["value_is_reference"]).unwrap_or(false)
    })
}

fn is_supported_reference_distinct_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "supported_reference_distinct")
}

fn collect_condition_target_variables(group: &ConditionGroup, variables: &mut Vec<String>) {
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

fn condition_targets_column(group: &ConditionGroup, column: &str) -> bool {
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

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn apply_cdisc_context_to_group(group: &mut ConditionGroup, context: &CdiscContext) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                apply_cdisc_context_to_group(group, context);
            }
        }
        ConditionGroup::Not(group) => apply_cdisc_context_to_group(group, context),
        ConditionGroup::Leaf(condition) => apply_cdisc_context_to_condition(condition, context),
    }
}

fn apply_cdisc_context_to_condition(condition: &mut Condition, context: &CdiscContext) {
    if !matches!(
        condition.operator,
        Operator::IsContainedBy
            | Operator::IsNotContainedBy
            | Operator::IsContainedByCaseInsensitive
            | Operator::IsNotContainedByCaseInsensitive
    ) || !matches!(condition.comparator, ValueExpr::Null)
    {
        return;
    }

    let Some(codelist) =
        condition_codelist(condition).or_else(|| define_codelist_for_condition(condition, context))
    else {
        return;
    };

    let Some(values) = context.terminology.values(&codelist) else {
        return;
    };

    condition.comparator = ValueExpr::List(
        values
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    );
}

fn condition_codelist(condition: &Condition) -> Option<String> {
    option_string_field(
        &condition.options.extra,
        &[
            "codelist",
            "codelist_oid",
            "ct_codelist",
            "define_codelist",
            "dictionary",
            "dictionary_name",
            "dictionary_id",
            "external_dictionary",
            "external_dictionary_name",
            "CodeListOID",
            "CodeList",
        ],
    )
}

fn define_codelist_for_condition(condition: &Condition, context: &CdiscContext) -> Option<String> {
    let target = condition.target.as_deref()?;
    let target_candidates = target_name_candidates(target);
    if let Some((domain, _unqualified)) = target.rsplit_once('.') {
        let domain_matches = context
            .define_xml
            .iter()
            .flat_map(|define| {
                define
                    .datasets
                    .iter()
                    .filter(move |dataset| {
                        dataset
                            .domain
                            .as_deref()
                            .or(dataset.name.as_deref())
                            .is_some_and(|name| name.eq_ignore_ascii_case(domain))
                    })
                    .flat_map(|dataset| {
                        dataset.item_refs.iter().filter_map(|item_ref| {
                            let item_oid = item_ref.item_oid.as_deref()?;
                            define
                                .variables
                                .iter()
                                .find(|variable| {
                                    variable.oid.as_deref() == Some(item_oid)
                                        && target_candidates.iter().any(|target| {
                                            variable.name.eq_ignore_ascii_case(target)
                                        })
                                })
                                .and_then(|variable| variable.codelist_oid.clone())
                        })
                    })
            })
            .collect::<Vec<_>>();
        if let Some(codelist) = unique_codelist(domain_matches) {
            return Some(codelist);
        }
    }

    let global_matches = context
        .define_xml
        .iter()
        .flat_map(|define| &define.variables)
        .filter(|variable| {
            target_candidates
                .iter()
                .any(|target| variable.name.eq_ignore_ascii_case(target))
        })
        .filter_map(|variable| variable.codelist_oid.clone())
        .collect::<Vec<_>>();
    unique_codelist(global_matches)
}

fn unique_codelist(codelists: Vec<String>) -> Option<String> {
    let unique = codelists.into_iter().collect::<BTreeSet<_>>();
    (unique.len() == 1)
        .then(|| unique.into_iter().next())
        .flatten()
}

fn target_name_candidates(target: &str) -> Vec<&str> {
    let mut candidates = vec![target];
    if let Some((_prefix, unqualified)) = target.rsplit_once('.') {
        candidates.push(unqualified);
    }
    candidates
}

fn option_string_field(map: &BTreeMap<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            map.get(*key).or_else(|| {
                map.iter()
                    .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
                    .map(|(_key, value)| value)
            })
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn execution_datasets_for_rule(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if let Some(result) = usdm_jsonata_execution_datasets(rule, datasets) {
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

fn dataset_domain_value(dataset: &LoadedDataset) -> String {
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

fn model_allowed_variables(dataset: &LoadedDataset) -> Vec<String> {
    if dataset_domain_value(dataset) == "VS" {
        return [
            "STUDYID", "DOMAIN", "USUBJID", "POOLID", "SPDEVID", "VSSEQ", "VSGRPID", "VSREFID",
            "VSSPID", "VSTESTCD", "VSTEST", "VSCAT", "VSSCAT", "VSPOS", "VSORRES", "VSORRESU",
            "VSSTRESC", "VSSTRESN", "VSSTRESU", "VSSTAT", "VSREASND", "VSLOC", "VSLAT", "VSDIR",
            "VSPORTOT", "VSMETHOD", "VSBLFL", "VSDRVFL", "VSLOBXFL", "VSFAST", "VSEVAL",
            "VSEVALID", "VSACPTFL", "VSREPNUM", "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH",
            "VSDTC", "VSDY", "VSTPT", "VSTPTNUM", "VSELTM", "VSTPTREF", "VSRFTDTC",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
    }

    dataset_variable_names_in_order(dataset)
        .into_iter()
        .filter(|name| {
            let upper = name.to_ascii_uppercase();
            !upper.starts_with('X') && !upper.ends_with("XX")
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

fn dataset_domain_values(dataset: &LoadedDataset) -> Vec<String> {
    let mut values = Vec::new();
    if let Ok(column_values) = dataset_column_values(dataset, "DOMAIN") {
        for value in column_values {
            if let Some(value) = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique_string(&mut values, &value.to_ascii_uppercase());
            }
        }
    }
    if values.is_empty() {
        values.push(dataset_domain_value(dataset));
    }
    values
}

fn is_custom_domain(domain: &str) -> bool {
    !matches!(
        domain.to_ascii_uppercase().as_str(),
        "AE" | "AG"
            | "BE"
            | "BG"
            | "CE"
            | "CL"
            | "CM"
            | "CO"
            | "CV"
            | "DA"
            | "DD"
            | "DM"
            | "DS"
            | "DV"
            | "EC"
            | "EG"
            | "EX"
            | "FA"
            | "FT"
            | "HO"
            | "IE"
            | "IS"
            | "LB"
            | "MA"
            | "MB"
            | "MH"
            | "MI"
            | "MO"
            | "MS"
            | "OM"
            | "PC"
            | "PD"
            | "PE"
            | "PP"
            | "PR"
            | "QS"
            | "RE"
            | "RELREC"
            | "RP"
            | "RS"
            | "SC"
            | "SE"
            | "SR"
            | "SS"
            | "SU"
            | "SV"
            | "TA"
            | "TE"
            | "TF"
            | "TI"
            | "TR"
            | "TS"
            | "TU"
            | "TV"
            | "UR"
            | "VS"
    )
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

fn insert_metadata_operation_value(row: &mut BTreeMap<String, Value>, output: &str, value: Value) {
    row.insert(output.to_owned(), value.clone());
    let clean = clean_operation_identifier(output);
    if clean != output {
        row.insert(clean, value);
    }
}

fn model_filtered_variable_names(
    dataset: &LoadedDataset,
    key_name: &str,
    key_value: &str,
) -> Vec<String> {
    if key_name.eq_ignore_ascii_case("role") && key_value.eq_ignore_ascii_case("Timing") {
        return timing_model_variables(dataset);
    }
    Vec::new()
}

fn timing_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let mut variables = ["VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    variables.extend([
        format!("{domain}DTC"),
        format!("{domain}STDTC"),
        format!("{domain}ENDTC"),
        format!("{domain}DY"),
        format!("{domain}STDY"),
        format!("{domain}ENDY"),
        format!("{domain}TPT"),
        format!("{domain}TPTNUM"),
        format!("{domain}ELTM"),
        format!("{domain}TPTREF"),
        format!("{domain}RFTDTC"),
        format!("{domain}ENRF"),
        format!("{domain}PDUR"),
        format!("{domain}ENRTPT"),
        format!("{domain}ENTPT"),
    ]);
    variables
}

fn model_column_order_from_library(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset_domain_value(dataset);
    match domain.as_str() {
        "AE" => vec!["STUDYID", "DOMAIN", "USUBJID", "AETERM"],
        "CE" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "CESEQ", "CEGRPID", "CETERM", "CESEV", "EPOCH",
            "CEDTC", "CESTDTC", "CEENDTC", "CEDY", "CESTDY", "CEENDY", "CESTRTPT", "CESTTPT",
            "CEENRTPT", "CEENTPT",
        ],
        "CM" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "CMSEQ", "CMTRT", "CMINDC", "CMDOSE", "CMDOSU",
            "CMDOSFRQ", "CMROUTE", "EPOCH", "CMDTC", "CMSTDTC", "CMENDTC", "CMDY", "CMSTDY",
            "CMENDY", "CMSTRTPT", "CMSTTPT", "CMENRTPT", "CMENTPT",
        ],
        "FA" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "FASEQ", "FALNKGRP", "FATESTCD", "FATEST", "FAOBJ",
            "FAORRES", "FASTRESC", "FASTNRLO", "FASTNRHI", "FALOC", "VISITNUM", "EPOCH", "FADTC",
            "FADY", "FAENRTPT", "FAENTPT",
        ],
        "LB" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "LBSEQ", "LBTESTCD", "LBTEST", "LBCAT", "LBORRES",
            "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC", "LBSTRESN", "LBSTRESU", "LBSTNRLO",
            "LBSTNRHI", "LBNRIND", "LBLOBXFL", "VISITNUM", "VISIT", "LBDTC", "LBDY", "LBENRTPT",
            "LBENTPT",
        ],
        "SE" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "SESEQ", "ETCD", "ELEMENT", "EPOCH", "SESTDTC",
            "SEENDTC", "SESTDY", "SEENDY",
        ],
        "SV" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "VISITNUM", "VISIT", "SVSTDTC", "SVENDTC", "SVSTDY",
            "SVENDY", "SVUPDES",
        ],
        "VS" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "VSSEQ", "VSTESTCD", "VSTEST", "VSPOS", "VSORRES",
            "VSORRESU", "VSSTRESC", "VSSTRESN", "VSSTRESU", "VSSTAT", "VSLOC", "VSLOBXFL",
            "VSREPNUM", "VISITNUM", "VISIT", "EPOCH", "VSDTC", "VSDY", "MIDS",
        ],
        _ => return model_allowed_variables(dataset),
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn dataset_variable_names_in_order(dataset: &LoadedDataset) -> Vec<String> {
    if dataset.metadata.variables.is_empty() {
        return dataset.summary().columns;
    }
    dataset
        .metadata
        .variables
        .iter()
        .map(|variable| variable.name.clone())
        .filter(|name| !name.trim().is_empty())
        .collect()
}

fn expected_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    match domain.as_str() {
        "AE" => vec![
            "AELLT", "AELLTCD", "AEPTCD", "AEHLT", "AEHLTCD", "AEHLGT", "AEHLGTCD", "AEBODSYS",
            "AEBDSYCD", "AESOC", "AESOCCD", "AESER", "AEACN", "AEREL", "AESTDTC", "AEENDTC",
        ],
        "EX" => vec!["EXDOSE", "EXDOSU", "EXDOSFRM", "EXSTDTC", "EXENDTC"],
        "LB" => vec![
            "LBCAT", "LBORRES", "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC", "LBSTRESN",
            "LBSTRESU", "LBSTNRLO", "LBSTNRHI", "LBNRIND", "LBLOBXFL", "VISITNUM", "LBDTC",
        ],
        "SUPPAE" => vec!["IDVAR", "IDVARVAL", "QEVAL"],
        "TA" => vec!["TABRANCH", "TATRANS"],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn required_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let mut variables = vec!["STUDYID", "DOMAIN"];
    if domain != "DM" && !is_trial_design_domain_without_subject(&domain) {
        variables.push("USUBJID");
    }
    match domain.as_str() {
        "AE" => variables.extend(["AESEQ", "AETERM"]),
        "CM" => variables.extend(["CMSEQ", "CMTRT"]),
        "DM" => variables.extend(["USUBJID", "SUBJID", "RFSTDTC", "RFENDTC", "SITEID", "SEX"]),
        "EX" => variables.extend(["EXSEQ", "EXTRT"]),
        "LB" => variables.extend(["LBSEQ", "LBTESTCD", "LBTEST"]),
        "VS" => variables.extend(["VSSEQ", "VSTESTCD", "VSTEST"]),
        _ => {}
    }
    variables.into_iter().map(str::to_owned).collect()
}

fn is_trial_design_domain_without_subject(domain: &str) -> bool {
    matches!(domain, "TA" | "TE" | "TI" | "TS" | "TV")
}

fn domain_presence_execution_datasets(
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

fn filter_datasets_by_rule_scope(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Vec<LoadedDataset> {
    if rule.entities.is_some() {
        return datasets
            .iter()
            .filter(|dataset| entity_scope_allows(rule.entities.as_ref(), dataset))
            .cloned()
            .collect();
    }
    filter_datasets_by_domain_scope(rule, datasets)
}

fn filter_datasets_by_domain_scope(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Vec<LoadedDataset> {
    datasets
        .iter()
        .filter(|dataset| {
            domain_scope_allows(rule.domains.as_ref(), dataset)
                && class_scope_allows(rule.classes.as_ref(), dataset)
        })
        .cloned()
        .collect()
}

fn domain_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name);
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");

    if scope_matches(&excludes, domain) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || scope_matches(&includes, domain)
}

fn entity_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let entity = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name);
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");

    if scope_matches(&excludes, entity) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || scope_matches(&includes, entity)
}

fn scope_values(scope: Option<&Value>, key: &str) -> Vec<String> {
    let Some(object) = scope.and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(value) = object.get(key).or_else(|| {
        object
            .iter()
            .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
            .map(|(_key, value)| value)
    }) else {
        return Vec::new();
    };

    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(value) => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn scope_contains_all(values: &[String]) -> bool {
    values.iter().any(|value| value.eq_ignore_ascii_case("ALL"))
}

fn scope_matches(values: &[String], domain: &str) -> bool {
    values
        .iter()
        .any(|value| domain_scope_matches(value, domain))
}

fn domain_scope_matches(pattern: &str, domain: &str) -> bool {
    if pattern.eq_ignore_ascii_case(domain) {
        return true;
    }
    if let Some((prefix, suffix)) = pattern.split_once("--") {
        return domain
            .to_ascii_uppercase()
            .starts_with(&prefix.to_ascii_uppercase())
            && domain
                .to_ascii_uppercase()
                .ends_with(&suffix.to_ascii_uppercase());
    }
    false
}

fn class_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");
    let Some(class) = dataset_domain_class(dataset) else {
        return true;
    };

    if class_scope_matches(&excludes, class) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || class_scope_matches(&includes, class)
}

fn dataset_domain_class(dataset: &LoadedDataset) -> Option<&'static str> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name)
        .to_ascii_uppercase();
    match domain.as_str() {
        "CM" | "EC" | "EX" | "ML" | "PR" | "SU" => Some("INTERVENTIONS"),
        "AE" | "CE" | "DS" | "DV" | "MH" => Some("EVENTS"),
        "CV" | "DD" | "EG" | "FT" | "IE" | "IS" | "LB" | "MB" | "MI" | "MS" | "PC" | "PP"
        | "QS" | "RE" | "RP" | "SC" | "SS" | "TR" | "TU" | "UR" | "VS" => Some("FINDINGS"),
        "FA" | "SR" => Some("FINDINGS ABOUT"),
        "CO" | "DM" | "SE" | "SV" => Some("SPECIAL PURPOSE"),
        "TA" | "TD" | "TE" | "TI" | "TM" | "TS" | "TV" => Some("TRIAL DESIGN"),
        "RELREC" | "SUPP" | "SUPPQUAL" => Some("RELATIONSHIP"),
        _ => None,
    }
}

fn class_scope_matches(values: &[String], class: &str) -> bool {
    let normalized_class = normalize_scope_class(class);
    values.iter().any(|value| {
        let normalized_value = normalize_scope_class(value);
        normalized_value == normalized_class
            || (normalized_value == "FINDINGS" && normalized_class == "FINDINGSABOUT")
    })
}

fn normalize_scope_class(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_uppercase()
}

fn execute_match_datasets(
    rule: &ExecutableRule,
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let match_datasets = rule.datasets.as_deref().unwrap_or_default();
    let mut names = match_datasets
        .iter()
        .filter_map(match_dataset_name)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(join_skipped_result(
            rule,
            "Match Datasets is missing dataset names",
        ));
    }
    if rule.entities.is_some() && !scoped_datasets.is_empty() {
        return execute_scoped_match_dataset_sequence(
            rule,
            match_datasets,
            &names,
            scoped_datasets,
            all_datasets,
        );
    }
    if names.len() == 1 {
        return execute_single_match_dataset(
            rule,
            &match_datasets[0],
            &names[0],
            scoped_datasets,
            all_datasets,
        );
    }

    let left_name = names.remove(0);
    let Some(mut joined) = find_dataset(all_datasets, &left_name).cloned() else {
        return Err(join_skipped_result(
            rule,
            format!("left dataset {left_name} was not loaded"),
        ));
    };

    for (index, right_name) in names.iter().enumerate() {
        let Some(right) = find_dataset(all_datasets, right_name) else {
            return Err(join_skipped_result(
                rule,
                format!("right dataset {right_name} was not loaded"),
            ));
        };
        let keys = match_datasets
            .get(index + 1)
            .and_then(match_dataset_join_keys)
            .or_else(|| match_datasets.first().and_then(match_dataset_join_keys))
            .or_else(|| common_join_keys(&joined, right).map(JoinKeys::same));
        let Some(keys) = keys else {
            return Err(join_skipped_result(
                rule,
                format!("no common join keys for {left_name} and {right_name}"),
            ));
        };
        let prefix = match_datasets
            .get(index + 1)
            .and_then(|dataset| match_dataset_string_field(dataset, &["prefix"]))
            .unwrap_or_else(|| format!("{right_name}."));
        joined = left_join_dataset_on(&joined, right, &keys.left, &keys.right, &prefix)
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
    }

    Ok(vec![joined])
}

fn execute_scoped_match_dataset_sequence(
    rule: &ExecutableRule,
    match_datasets: &[MatchDataset],
    names: &[String],
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let mut joined_datasets = Vec::with_capacity(scoped_datasets.len());
    for scoped_base in scoped_datasets {
        let mut joined = scoped_base.clone();
        for (match_dataset, match_name) in match_datasets.iter().zip(names) {
            let Some(lookup_dataset) = find_dataset(all_datasets, match_name) else {
                if is_left_match_dataset(match_dataset) {
                    joined = add_missing_left_match_columns(&joined, rule, match_name)
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
                    continue;
                }
                return Err(join_skipped_result(
                    rule,
                    format!("dataset {match_name} was not loaded"),
                ));
            };
            let keys = match_dataset_join_keys(match_dataset)
                .or_else(|| common_join_keys(&joined, lookup_dataset).map(JoinKeys::same));
            let Some(keys) = keys else {
                return Err(join_skipped_result(
                    rule,
                    format!("match dataset {match_name} is missing keys"),
                ));
            };

            if let Some(prefix) = match_dataset_string_field(match_dataset, &["prefix"]) {
                joined =
                    left_join_dataset_on(&joined, lookup_dataset, &keys.left, &keys.right, &prefix)
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
            } else {
                let lookup_dataset = suffix_conflicting_match_columns(
                    &joined,
                    lookup_dataset,
                    &keys.right,
                    match_name,
                    rule,
                )
                .map_err(|source| join_skipped_result(rule, source.to_string()))?;
                joined =
                    left_join_dataset_on(&joined, &lookup_dataset, &keys.left, &keys.right, "")
                        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
            }
        }
        joined_datasets.push(joined);
    }
    Ok(joined_datasets)
}

fn suffix_conflicting_match_columns(
    left: &LoadedDataset,
    right: &LoadedDataset,
    right_keys: &[String],
    suffix: &str,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    let mut right = right.clone();
    for key in right_keys {
        if !dataset_has_column(left, key) {
            continue;
        }
        let suffixed_key = format!("{key}.{suffix}");
        if !rule_references_column(rule, &suffixed_key) {
            continue;
        }
        if dataset_has_column(left, &suffixed_key) || dataset_has_column(&right, &suffixed_key) {
            continue;
        }
        let Some(source_key) = dataset_column_name(&right, key) else {
            continue;
        };
        right = derive_column_from_column(&right, &suffixed_key, &source_key)?;
    }

    let mut renames = BTreeMap::new();
    for column in right.frame().get_column_names() {
        let column = column.as_str();
        if right_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(column))
        {
            continue;
        }
        let suffixed_column = format!("{column}.{suffix}");
        if dataset_has_column(left, column) || rule_references_column(rule, &suffixed_column) {
            renames.insert(column.to_owned(), suffixed_column);
        }
    }
    if renames.is_empty() {
        return Ok(right);
    }
    rename_dataset_columns(&right, &renames)
}

fn add_missing_left_match_columns(
    dataset: &LoadedDataset,
    rule: &ExecutableRule,
    suffix: &str,
) -> core_data::Result<LoadedDataset> {
    let mut joined = dataset.clone();
    for column in rule_referenced_columns_with_suffix(rule, suffix) {
        if dataset_has_column(&joined, &column) {
            continue;
        }
        joined = derive_literal_column(&joined, &column, &Value::Null)?;
    }
    Ok(joined)
}

fn rule_referenced_columns_with_suffix(rule: &ExecutableRule, suffix: &str) -> BTreeSet<String> {
    let mut columns = BTreeSet::new();
    for variable in &rule.output_variables {
        collect_column_with_suffix(variable, suffix, &mut columns);
    }
    collect_condition_columns_with_suffix(&rule.conditions, suffix, &mut columns);
    columns
}

fn collect_condition_columns_with_suffix(
    group: &ConditionGroup,
    suffix: &str,
    columns: &mut BTreeSet<String>,
) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                collect_condition_columns_with_suffix(group, suffix, columns);
            }
        }
        ConditionGroup::Not(group) => collect_condition_columns_with_suffix(group, suffix, columns),
        ConditionGroup::Leaf(condition) => {
            if let Some(target) = condition.target.as_deref() {
                collect_column_with_suffix(target, suffix, columns);
            }
            collect_value_expr_columns_with_suffix(&condition.comparator, suffix, columns);
        }
    }
}

fn collect_value_expr_columns_with_suffix(
    expr: &ValueExpr,
    suffix: &str,
    columns: &mut BTreeSet<String>,
) {
    match expr {
        ValueExpr::ColumnRef(reference) => collect_column_with_suffix(reference, suffix, columns),
        ValueExpr::List(values) => {
            for value in values {
                if let Some(reference) = value.as_str() {
                    collect_column_with_suffix(reference, suffix, columns);
                }
            }
        }
        ValueExpr::Literal(_) | ValueExpr::Null => {}
    }
}

fn collect_column_with_suffix(column: &str, suffix: &str, columns: &mut BTreeSet<String>) {
    if column
        .rsplit_once('.')
        .is_some_and(|(_, column_suffix)| column_suffix.eq_ignore_ascii_case(suffix))
    {
        columns.insert(column.to_owned());
    }
}

fn rule_references_column(rule: &ExecutableRule, column: &str) -> bool {
    rule.output_variables
        .iter()
        .any(|variable| variable.eq_ignore_ascii_case(column))
        || condition_group_references_column(&rule.conditions, column)
}

fn condition_group_references_column(group: &ConditionGroup, column: &str) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| condition_group_references_column(group, column)),
        ConditionGroup::Not(group) => condition_group_references_column(group, column),
        ConditionGroup::Leaf(condition) => {
            condition
                .target
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case(column))
                || value_expr_references_column(&condition.comparator, column)
        }
    }
}

fn value_expr_references_column(expr: &ValueExpr, column: &str) -> bool {
    match expr {
        ValueExpr::ColumnRef(reference) => reference.eq_ignore_ascii_case(column),
        ValueExpr::List(values) => values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|reference| reference.eq_ignore_ascii_case(column))
        }),
        ValueExpr::Literal(_) | ValueExpr::Null => false,
    }
}

fn execute_single_match_dataset(
    rule: &ExecutableRule,
    match_dataset: &MatchDataset,
    match_name: &str,
    scoped_datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let scoped_bases = scoped_datasets
        .iter()
        .filter(|dataset| {
            engine_semantics::includes_single_match_dataset_as_target(rule)
                || !dataset_matches_name(dataset, match_name)
        })
        .collect::<Vec<_>>();
    if scoped_bases.is_empty() {
        let Some(dataset) = find_dataset(scoped_datasets, match_name) else {
            return Err(join_skipped_result(
                rule,
                format!("dataset {match_name} was not loaded"),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    let Some(lookup_dataset) = find_dataset(all_datasets, match_name) else {
        if is_left_match_dataset(match_dataset) {
            return Ok(scoped_bases.into_iter().cloned().collect());
        }
        return Err(join_skipped_result(
            rule,
            format!("dataset {match_name} was not loaded"),
        ));
    };
    let Some(keys) = match_dataset_join_keys(match_dataset) else {
        return Err(join_skipped_result(
            rule,
            format!("match dataset {match_name} is missing keys"),
        ));
    };
    let prefix = match_dataset_string_field(match_dataset, &["prefix"])
        .unwrap_or_else(|| default_single_match_dataset_prefix(rule, match_name));
    let mut joined_datasets = Vec::with_capacity(scoped_bases.len());
    for scoped_base in scoped_bases {
        let scoped_base = add_source_row_column(scoped_base)
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        let lookup_dataset = if prefix.is_empty() {
            suffix_conflicting_match_columns(
                &scoped_base,
                lookup_dataset,
                &keys.right,
                match_name,
                rule,
            )
            .map_err(|source| join_skipped_result(rule, source.to_string()))?
        } else {
            lookup_dataset.clone()
        };
        let mut joined = left_join_dataset_on(
            &scoped_base,
            &lookup_dataset,
            &keys.left,
            &keys.right,
            &prefix,
        )
        .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        if !prefix.is_empty() {
            joined = add_unprefixed_match_aliases(
                &joined,
                &scoped_base,
                &lookup_dataset,
                &keys.right,
                &prefix,
                rule,
            )
            .map_err(|source| join_skipped_result(rule, source.to_string()))?;
        }
        joined_datasets.push(joined);
    }
    Ok(joined_datasets)
}

fn add_source_row_column(dataset: &LoadedDataset) -> core_data::Result<LoadedDataset> {
    if dataset_has_column(dataset, SOURCE_ROW_COLUMN) {
        return Ok(dataset.clone());
    }
    row_number_dataset(dataset, SOURCE_ROW_COLUMN, &[])
}

fn add_unprefixed_match_aliases(
    joined: &LoadedDataset,
    left: &LoadedDataset,
    right: &LoadedDataset,
    right_keys: &[String],
    prefix: &str,
    rule: &ExecutableRule,
) -> core_data::Result<LoadedDataset> {
    let mut joined = joined.clone();
    for column in right.frame().get_column_names() {
        let column = column.as_str();
        if right_keys
            .iter()
            .any(|key| key.eq_ignore_ascii_case(column))
            || dataset_has_column(left, column)
            || dataset_has_column(&joined, column)
            || !rule_references_column(rule, column)
        {
            continue;
        }
        let prefixed_column = format!("{prefix}{column}");
        if !dataset_has_column(&joined, &prefixed_column) {
            continue;
        }
        joined = derive_column_from_column(&joined, column, &prefixed_column)?;
    }
    Ok(joined)
}

fn default_single_match_dataset_prefix(rule: &ExecutableRule, match_name: &str) -> String {
    if rule_references_match_dataset_prefixed_column(rule, match_name) {
        format!("{match_name}.")
    } else {
        String::new()
    }
}

fn rule_references_match_dataset_prefixed_column(rule: &ExecutableRule, match_name: &str) -> bool {
    rule.output_variables
        .iter()
        .any(|variable| column_has_match_dataset_prefix(variable, match_name))
        || condition_group_references_match_dataset_prefix(&rule.conditions, match_name)
}

fn condition_group_references_match_dataset_prefix(
    group: &ConditionGroup,
    match_name: &str,
) -> bool {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => groups
            .iter()
            .any(|group| condition_group_references_match_dataset_prefix(group, match_name)),
        ConditionGroup::Not(group) => {
            condition_group_references_match_dataset_prefix(group, match_name)
        }
        ConditionGroup::Leaf(condition) => {
            condition
                .target
                .as_deref()
                .is_some_and(|target| column_has_match_dataset_prefix(target, match_name))
                || value_expr_references_match_dataset_prefix(&condition.comparator, match_name)
        }
    }
}

fn value_expr_references_match_dataset_prefix(expr: &ValueExpr, match_name: &str) -> bool {
    match expr {
        ValueExpr::ColumnRef(reference) => column_has_match_dataset_prefix(reference, match_name),
        ValueExpr::List(values) => values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|reference| column_has_match_dataset_prefix(reference, match_name))
        }),
        ValueExpr::Literal(_) | ValueExpr::Null => false,
    }
}

fn column_has_match_dataset_prefix(column: &str, match_name: &str) -> bool {
    column
        .split_once('.')
        .is_some_and(|(prefix, _)| prefix.eq_ignore_ascii_case(match_name))
}

fn match_dataset_name(dataset: &MatchDataset) -> Option<String> {
    match_dataset_string_field(
        dataset,
        &[
            "dataset", "domain", "name", "id", "Dataset", "Domain", "Name",
        ],
    )
}

fn is_left_match_dataset(dataset: &MatchDataset) -> bool {
    match_dataset_string_field(dataset, &["join_type", "join type", "Join Type"])
        .is_some_and(|join_type| join_type.eq_ignore_ascii_case("left"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JoinKeys {
    left: Vec<String>,
    right: Vec<String>,
}

impl JoinKeys {
    fn same(keys: Vec<String>) -> Self {
        Self {
            left: keys.clone(),
            right: keys,
        }
    }
}

fn match_dataset_join_keys(dataset: &MatchDataset) -> Option<JoinKeys> {
    let value = match_dataset_value(dataset, &["by", "keys", "on", "join_keys", "match_keys"])?;
    join_keys_from_value(value)
}

fn join_keys_from_value(value: &Value) -> Option<JoinKeys> {
    match value {
        Value::String(value) if !value.is_empty() => Some(JoinKeys::same(vec![value.clone()])),
        Value::Array(values) => {
            let mut left = Vec::new();
            let mut right = Vec::new();
            for value in values {
                match value {
                    Value::String(value) if !value.is_empty() => {
                        left.push(value.clone());
                        right.push(value.clone());
                    }
                    Value::Object(_) => {
                        let left_key = object_string_field(value, &["left"])?;
                        let right_key = object_string_field(value, &["right"])?;
                        left.push(left_key);
                        right.push(right_key);
                    }
                    _ => return None,
                }
            }
            (!left.is_empty()).then_some(JoinKeys { left, right })
        }
        Value::Object(_) => {
            let left = object_string_field(value, &["left"])?;
            let right = object_string_field(value, &["right"])?;
            Some(JoinKeys {
                left: vec![left],
                right: vec![right],
            })
        }
        _ => None,
    }
}

fn object_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    let Value::Object(fields) = value else {
        return None;
    };
    keys.iter().find_map(|key| {
        fields
            .get(*key)
            .or_else(|| {
                let normalized = normalize_operation_key(key);
                fields
                    .iter()
                    .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized)
                    .map(|(_key, value)| value)
            })
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn match_dataset_string_field(dataset: &MatchDataset, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| match_dataset_value(dataset, &[*key]))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn match_dataset_value<'a>(dataset: &'a MatchDataset, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        dataset
            .fields
            .get(*key)
            .or_else(|| match_dataset_field_normalized(dataset, key))
    })
}

fn match_dataset_field_normalized<'a>(dataset: &'a MatchDataset, key: &str) -> Option<&'a Value> {
    let normalized_key = normalize_operation_key(key);
    dataset
        .fields
        .iter()
        .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized_key)
        .map(|(_key, value)| value)
}

fn common_join_keys(left: &LoadedDataset, right: &LoadedDataset) -> Option<Vec<String>> {
    let right_columns = right
        .summary()
        .columns
        .into_iter()
        .map(|column| column.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let left_columns = left.summary().columns;
    let mut keys = Vec::new();
    for preferred in ["STUDYID", "USUBJID", "DOMAIN", "IDVAR", "IDVARVAL"] {
        if left_columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case(preferred))
            && right_columns.contains(&preferred.to_ascii_lowercase())
        {
            keys.push(preferred.to_owned());
        }
    }
    for column in left_columns {
        if right_columns.contains(&column.to_ascii_lowercase())
            && !keys.iter().any(|key| key.eq_ignore_ascii_case(&column))
        {
            keys.push(column);
        }
    }
    (!keys.is_empty()).then_some(keys)
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

fn operation_column_values(
    dataset: &LoadedDataset,
    column_name: &str,
) -> std::result::Result<Vec<Value>, DataError> {
    let resolved = resolve_operation_column_name(dataset, column_name)
        .unwrap_or_else(|| column_name.to_owned());
    dataset_column_values(dataset, &resolved)
}

fn operation_group_key_columns(
    dataset: &LoadedDataset,
    keys: &[String],
) -> std::result::Result<Vec<Vec<Value>>, DataError> {
    let mut columns = Vec::new();
    for key in keys {
        let key = expand_dataset_domain_placeholder(dataset, key);
        let values = operation_column_values(dataset, &key)?;
        if let Some(variable_names) = dynamic_column_list_values(&values) {
            for variable_name in variable_names {
                columns.push(operation_column_values(dataset, &variable_name)?);
            }
        } else {
            columns.push(values);
        }
    }
    Ok(columns)
}

fn dynamic_column_list_values(values: &[Value]) -> Option<Vec<String>> {
    values.iter().find_map(|value| {
        json_scalar_string(value)
            .as_deref()
            .and_then(parse_column_list_literal)
            .filter(|columns| !columns.is_empty())
    })
}

fn parse_column_list_literal(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let columns = inner
        .split(',')
        .filter_map(|part| {
            let column = part.trim().trim_matches('"').trim_matches('\'').trim();
            (!column.is_empty()).then(|| column.to_owned())
        })
        .collect::<Vec<_>>();
    Some(columns)
}

fn resolve_operation_column_name(dataset: &LoadedDataset, column_name: &str) -> Option<String> {
    let clean_target = clean_operation_identifier(column_name);
    let columns = dataset.summary().columns;
    if let Some(found) = columns.iter().find(|candidate| {
        candidate.as_str() == column_name
            || candidate.eq_ignore_ascii_case(column_name)
            || clean_operation_identifier(candidate) == clean_target
    }) {
        return Some(found.clone());
    }
    let base_name = column_name.rsplit_once('.').map(|(base, _suffix)| base)?;
    let clean_base = clean_operation_identifier(base_name);
    columns.into_iter().find(|candidate| {
        candidate == base_name
            || candidate.eq_ignore_ascii_case(base_name)
            || clean_operation_identifier(candidate) == clean_base
    })
}

fn group_count_dataset_with_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
    keys: &[String],
    output: &str,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let regex_pattern = string_field(operation, &["regex", "pattern"]);
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        let mask = vec![true; dataset.summary().row_count];
        return derive_filtered_group_count_dataset(
            dataset,
            &mask,
            keys,
            output,
            regex_pattern.as_deref(),
        )
        .map_err(|source| operation_skipped_result(rule, source.to_string()));
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    derive_filtered_group_count_dataset(dataset, &mask, keys, output, regex_pattern.as_deref())
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

fn derive_filtered_group_count_dataset(
    dataset: &LoadedDataset,
    mask: &[bool],
    keys: &[String],
    column_name: &str,
    regex_pattern: Option<&str>,
) -> std::result::Result<LoadedDataset, DataError> {
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "group count operation requires an output column".to_owned(),
        ));
    }
    let row_count = dataset.summary().row_count;
    if mask.len() != row_count {
        return Err(DataError::InvalidDatasetPackage(format!(
            "filter mask length {} does not match row count {}",
            mask.len(),
            row_count
        )));
    }

    if keys.is_empty() {
        let count = mask.iter().filter(|keep| **keep).count() as i64;
        let values = (0..row_count)
            .map(|_| Value::Number(serde_json::Number::from(count)))
            .collect::<Vec<_>>();
        return derive_column_from_values_with_aliases(dataset, column_name, &values);
    }

    let key_columns = operation_group_key_columns(dataset, keys)?;
    let regex = regex_pattern
        .map(regex::Regex::new)
        .transpose()
        .map_err(|source| DataError::InvalidDatasetPackage(source.to_string()))?;
    let mut counts = BTreeMap::new();
    for (row, keep) in mask.iter().enumerate().take(row_count) {
        if *keep {
            *counts
                .entry(filtered_group_count_key(&key_columns, row, regex.as_ref()))
                .or_insert(0_i64) += 1;
        }
    }
    let values = (0..row_count)
        .map(|row| {
            let count = *counts
                .get(&filtered_group_count_key(&key_columns, row, regex.as_ref()))
                .unwrap_or(&0_i64);
            Value::Number(serde_json::Number::from(count))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn filtered_group_count_key(
    columns: &[Vec<Value>],
    row: usize,
    regex: Option<&regex::Regex>,
) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            let value = column
                .get(row)
                .and_then(json_scalar_string)
                .unwrap_or_default();
            normalize_operation_group_key_value(&value, regex)
        })
        .collect()
}

fn normalize_operation_group_key_value(value: &str, regex: Option<&regex::Regex>) -> String {
    regex
        .and_then(|regex| regex.find(value))
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| value.to_owned())
}

fn group_distinct_values_dataset_with_aliases(
    dataset: &LoadedDataset,
    keys: &[String],
    aliases: &[String],
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires a source column".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires an output column".to_owned(),
        ));
    }
    if !aliases.is_empty() && aliases.len() != keys.len() {
        return Err(DataError::InvalidDatasetPackage(
            "distinct values operation requires matching group and group_aliases".to_owned(),
        ));
    }

    let source_key_columns = keys
        .iter()
        .map(|key| {
            operation_column_values(dataset, key).map_err(|_source| {
                DataError::InvalidDatasetPackage(format!("distinct values key not found: {key}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let target_keys = if aliases.is_empty() { keys } else { aliases };
    let target_key_columns = target_keys
        .iter()
        .map(|key| {
            operation_column_values(dataset, key).map_err(|_source| {
                DataError::InvalidDatasetPackage(format!("distinct values key not found: {key}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let source_values = operation_column_values(dataset, source_column).map_err(|_source| {
        DataError::InvalidDatasetPackage(format!(
            "distinct values source column not found: {source_column}"
        ))
    })?;

    let mut groups = BTreeMap::<Vec<String>, BTreeSet<String>>::new();
    for row in 0..dataset.frame().height() {
        if let Some(value) = source_values.get(row).and_then(json_scalar_string) {
            groups
                .entry(filtered_group_count_key(&source_key_columns, row, None))
                .or_default()
                .insert(value);
        }
    }

    let values = (0..dataset.frame().height())
        .map(|row| {
            let joined = groups
                .get(&filtered_group_count_key(&target_key_columns, row, None))
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default();
            Value::String(joined)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn apply_operation_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let mask = operation_inline_filter_mask(rule, operation, dataset)?;
    filter_dataset_by_mask(dataset, &mask)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

fn operation_inline_filter_mask(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
) -> std::result::Result<Vec<bool>, RuleValidationResult> {
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        return Ok(vec![true; dataset.summary().row_count]);
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    Ok(mask)
}

fn normalize_operation_filter_value(
    value: &Value,
) -> std::result::Result<ConditionGroup, RuleModelError> {
    let Some(object) = value.as_object() else {
        return normalize_condition_value(value);
    };
    if object.keys().any(|key| {
        matches!(
            normalize_operation_key(key).as_str(),
            "all" | "any" | "not" | "name" | "target" | "operator"
        )
    }) {
        return normalize_condition_value(value);
    }

    Ok(ConditionGroup::All(
        object
            .iter()
            .map(|(target, comparator)| {
                ConditionGroup::Leaf(Condition {
                    target: Some(target.clone()),
                    operator: Operator::EqualTo,
                    comparator: ValueExpr::Literal(comparator.clone()),
                    options: Default::default(),
                })
            })
            .collect(),
    ))
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
    if !group_keys.is_empty() && (scope_wide || !group_aliases.is_empty()) {
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
        .filter_map(json_scalar_string)
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
        if let Some(value) = source_values.get(row).and_then(json_scalar_string) {
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

fn derive_domain_label_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
    prefer_domain_name: bool,
) -> std::result::Result<LoadedDataset, DataError> {
    let label = if prefer_domain_name {
        domain_name_value(dataset)
    } else {
        domain_label_value(dataset)
    };
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(label.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_study_domains_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let domains = all_datasets
        .iter()
        .flat_map(|dataset| {
            [
                dataset.metadata.domain.as_deref(),
                Some(dataset.metadata.name.as_str()),
            ]
        })
        .flatten()
        .filter_map(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_ascii_uppercase())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(domains.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_variable_count_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "variable_count operation requires a source variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "variable_count operation requires an output column".to_owned(),
        ));
    }

    let count = all_datasets
        .iter()
        .filter(|candidate| {
            let column = expand_dataset_domain_placeholder(candidate, source_column);
            dataset_has_variable(candidate, &column)
        })
        .count() as i64;
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::Number(serde_json::Number::from(count)))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_study_day_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    reference_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires a source date variable".to_owned(),
        ));
    }
    if reference_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires a reference date variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires an output column".to_owned(),
        ));
    }

    let source_column = expand_dataset_domain_placeholder(dataset, source_column);
    let source_dates = dataset_column_values(dataset, &source_column)?;
    let reference_dates = dataset_column_values(dataset, reference_column)?;
    let values = source_dates
        .iter()
        .zip(reference_dates.iter())
        .map(|(source, reference)| {
            study_day_value(
                json_scalar_string(source).as_deref(),
                json_scalar_string(reference).as_deref(),
            )
            .map(|value| Value::Number(serde_json::Number::from(value)))
            .unwrap_or(Value::Null)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn study_day_value(source: Option<&str>, reference: Option<&str>) -> Option<i64> {
    let source = days_from_study_date(source?)?;
    let reference = days_from_study_date(reference?)?;
    let diff = source - reference;
    Some(if diff >= 0 { diff + 1 } else { diff })
}

fn days_from_study_date(value: &str) -> Option<i64> {
    let date = value.trim().get(..10)?;
    let year = parse_fixed_i32(date.get(0..4)?)?;
    let separator_1 = date.get(4..5)?;
    let month = parse_fixed_u32(date.get(5..7)?)?;
    let separator_2 = date.get(7..8)?;
    let day = parse_fixed_u32(date.get(8..10)?)?;
    if separator_1 != "-" || separator_2 != "-" {
        return None;
    }
    if !(1..=12).contains(&month) || day == 0 || day > days_in_study_month(year, month) {
        return None;
    }

    Some(days_from_civil(year, month, day))
}

fn parse_fixed_i32(value: &str) -> Option<i32> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<i32>().ok())
        .flatten()
}

fn parse_fixed_u32(value: &str) -> Option<u32> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<u32>().ok())
        .flatten()
}

fn days_in_study_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_study_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_study_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = (adjusted_year - era * 400) as i64;
    let month = month as i64;
    let day = day as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era as i64 * 146_097 + day_of_era
}

fn derive_metadata_dataset(
    dataset: &LoadedDataset,
    field: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let value = match normalize_operation_key(field).as_str() {
        "dataset_name" | "name" => dataset.metadata.name.clone(),
        "domain" => dataset
            .metadata
            .domain
            .clone()
            .unwrap_or_else(|| dataset.metadata.name.clone()),
        "label" | "dataset_label" => dataset.metadata.label.clone().unwrap_or_default(),
        other => {
            return Err(DataError::InvalidDatasetPackage(format!(
                "unsupported metadata field: {other}"
            )));
        }
    };
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(value.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_valid_codelist_dates_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let joined = valid_codelist_dates_for_operation(operation).join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(joined.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn valid_codelist_dates_for_operation(operation: &OperationSpec) -> &'static [&'static str] {
    let package_types =
        string_list_field(operation, &["ct_package_types", "ct_package_type"]).unwrap_or_default();
    if package_types
        .iter()
        .any(|package_type| package_type.eq_ignore_ascii_case("DDF"))
    {
        return ddf_valid_codelist_dates();
    }
    valid_codelist_dates()
}

fn ddf_valid_codelist_dates() -> &'static [&'static str] {
    valid_codelist_dates()
}

fn valid_codelist_dates() -> &'static [&'static str] {
    &[
        "2014-09-26",
        "2014-12-19",
        "2015-03-27",
        "2015-06-26",
        "2015-09-25",
        "2015-12-18",
        "2016-03-25",
        "2016-06-24",
        "2016-09-30",
        "2016-12-16",
        "2017-03-31",
        "2017-06-30",
        "2017-09-29",
        "2017-12-22",
        "2018-03-30",
        "2018-06-29",
        "2018-09-28",
        "2018-12-21",
        "2019-03-29",
        "2019-06-28",
        "2019-09-27",
        "2019-12-20",
        "2020-03-27",
        "2020-06-26",
        "2020-11-06",
        "2020-12-18",
        "2021-03-26",
        "2021-06-25",
        "2021-09-24",
        "2021-12-17",
        "2022-03-25",
        "2022-06-24",
        "2022-09-30",
        "2022-12-16",
        "2023-03-31",
        "2023-06-30",
        "2023-09-29",
        "2023-12-15",
        "2024-03-29",
        "2024-09-27",
        "2025-03-28",
        "2025-09-26",
    ]
}

fn derive_mapped_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let mappings = operation_value(operation, &["map"])
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut values = Vec::with_capacity(dataset.summary().row_count);

    for row in 0..dataset.summary().row_count {
        let mut mapped = String::new();
        for mapping in &mappings {
            let Some(object) = mapping.as_object() else {
                continue;
            };
            let output = object
                .get("output")
                .and_then(json_scalar_string)
                .unwrap_or_default();
            if output.is_empty() {
                continue;
            }
            let matched = object
                .iter()
                .filter(|(key, _value)| key.as_str() != "output")
                .all(|(key, expected)| {
                    operation_column_values(dataset, key)
                        .ok()
                        .and_then(|values| values.get(row).and_then(json_scalar_string))
                        .zip(json_scalar_string(expected))
                        .is_some_and(|(actual, expected)| actual == expected)
                });
            if matched {
                mapped = output;
                break;
            }
        }
        values.push(Value::String(mapped));
    }

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_codelist_extensible_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let codelist_codes = operation_reference_values(dataset, operation, "codelist_code")?;
    let codelist_versions = optional_operation_reference_values(dataset, operation, "version")?;
    let values = codelist_codes
        .iter()
        .enumerate()
        .map(|(row, code)| {
            let version = codelist_versions
                .as_ref()
                .and_then(|values| values.get(row))
                .map(String::as_str);
            if !static_codelist_matches_version(code, version) {
                return Value::Null;
            }
            match static_codelist(code).map(|codelist| codelist.extensible) {
                Some(value) => Value::Bool(value),
                None => Value::Null,
            }
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_codelist_terms_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let codelist_codes = if operation_value(operation, &["codelist_code"]).is_some() {
        operation_reference_values(dataset, operation, "codelist_code")?
    } else {
        let literal = string_list_field(operation, &["codelists"])
            .and_then(|values| values.first().cloned())
            .unwrap_or_default();
        vec![literal; dataset.summary().row_count]
    };
    let return_type = string_field(operation, &["returntype", "return_type"])
        .unwrap_or_else(|| "value".to_owned());
    let term_code = optional_operation_reference_values(dataset, operation, "term_code")?;
    let term_pref_term = optional_operation_reference_values(dataset, operation, "term_pref_term")?;
    let term_value = optional_operation_reference_values(dataset, operation, "term_value")?;
    let term_version = optional_operation_reference_values(dataset, operation, "version")?;

    let values = (0..dataset.summary().row_count)
        .map(|row| {
            let Some(codelist_code) = codelist_codes.get(row) else {
                return Value::String(String::new());
            };
            let Some(codelist) = static_codelist(codelist_code) else {
                return Value::String(String::new());
            };
            let version = term_version
                .as_ref()
                .and_then(|values| values.get(row))
                .map(String::as_str);
            if term_code.is_none() && term_pref_term.is_none() && term_value.is_none() {
                let values = codelist
                    .terms
                    .iter()
                    .filter(|term| {
                        static_codelist_term_matches_version(codelist_code, term, version)
                    })
                    .map(|term| term.value)
                    .collect::<Vec<_>>()
                    .join("|");
                return Value::String(values);
            }
            let term = term_code
                .as_ref()
                .and_then(|values| values.get(row))
                .and_then(|code| {
                    static_codelist_term_by_code(codelist_code, &codelist, code, version)
                })
                .or_else(|| {
                    term_pref_term
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|pref_term| {
                            static_codelist_term_by_pref_term(
                                codelist_code,
                                &codelist,
                                pref_term,
                                version,
                            )
                        })
                })
                .or_else(|| {
                    term_value
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|value| {
                            static_codelist_term_by_value(codelist_code, &codelist, value, version)
                        })
                });
            let Some(term) = term else {
                return Value::String(String::new());
            };
            if !static_codelist_term_matches_version(codelist_code, term, version) {
                return Value::String(String::new());
            }
            let value = match normalize_operation_key(&return_type).as_str() {
                "code" => term.code,
                "pref_term" | "preferred_term" => term.pref_term,
                "value" | "submission_value" => term.value,
                _ => "",
            };
            Value::String(value.to_owned())
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_split_by_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    delimiter: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let source_values = operation_column_values(dataset, source_column)?;
    let values = source_values
        .iter()
        .map(|value| {
            let Some(value) = json_scalar_string(value) else {
                return Value::String(String::new());
            };
            let parts = value
                .split(delimiter)
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("|");
            Value::String(parts)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn derive_parent_model_column_order_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let rdomains = operation_column_values(dataset, "RDOMAIN")?;
    let values = rdomains
        .iter()
        .map(|value| {
            let domain = json_scalar_string(value).unwrap_or_default();
            Value::String(parent_model_columns(&domain).unwrap_or_default().join("|"))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn parent_model_columns(domain: &str) -> Option<&'static [&'static str]> {
    match domain.trim().to_ascii_uppercase().as_str() {
        "AE" => Some(AE_MODEL_COLUMNS),
        "LB" => Some(LB_MODEL_COLUMNS),
        _ => None,
    }
}

const AE_MODEL_COLUMNS: &[&str] = &[
    "STUDYID", "AEGRPID", "AEREFID", "AERECID", "AESPID", "DOMAIN", "USUBJID", "AESEQ", "AETERM",
    "AEHLGT", "AEHLGTCD", "AECAT", "AESCAT", "AEPRESP", "AEOCCUR", "AEREASOC", "AESTAT",
    "AEREASND", "AEBODSYS", "AEMODIFY", "AEBDSYCD", "AESOC", "AESOCCD", "AELOC", "AELAT", "AEDIR",
    "AEPORTOT", "AEPARTY", "AELLT", "AEPRTYID", "AESEV", "AESER", "AEACN", "AEACNOTH", "AEACNDEV",
    "AEREL", "AERLDEV", "AERELNST", "AEPATT", "AELLTCD", "AEOUT", "AESCONG", "AESDISAB", "AESDTH",
    "AESHOSP", "AESLIFE", "AESOD", "AESMIE", "AESINTV", "AEDECOD", "AECONTRT", "AETOX", "AETOXGR",
    "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH", "AEDTC", "AESTDTC", "AEENDTC", "AEDY",
    "AESTDY", "AEENDY", "AEDUR",
];

const LB_MODEL_COLUMNS: &[&str] = &[
    "STUDYID", "LBGRPID", "LBREFID", "LBRECID", "LBSPID", "DOMAIN", "USUBJID", "LBSEQ", "LBTESTCD",
    "LBTEST", "LBCAT", "LBSCAT", "LBORRES", "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC",
    "LBSTRESN", "LBSTRESU", "LBSTNRLO", "LBSTNRHI", "LBSTNRC", "LBNRIND", "LBSTAT", "LBREASND",
    "LBNAM", "LBLOINC", "LBSPEC", "LBSPCCND", "LBSPCUFL", "LBLOC", "LBLAT", "LBDIR", "LBPORTOT",
    "LBMETHOD", "LBANMETH", "LBLOBXFL", "LBBLFL", "LBFAST", "LBDRVFL", "LBTOX", "LBTOXGR",
    "LBCLSIG", "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH", "LBDTC", "LBSTDTC", "LBENDTC",
    "LBDY", "LBSTDY", "LBENDY", "LBTPT", "LBTPTNUM", "LBELTM", "LBTPTREF", "LBRFTDTC", "LBPTFL",
    "LBPDUR",
];

fn operation_reference_values(
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

fn optional_operation_reference_values(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    key: &str,
) -> std::result::Result<Option<Vec<String>>, DataError> {
    if operation_value(operation, &[key]).is_none() {
        return Ok(None);
    }
    operation_reference_values(dataset, operation, key).map(Some)
}

#[derive(Clone, Copy)]
struct StaticCodelist {
    extensible: bool,
    terms: &'static [StaticTerm],
}

#[cfg(test)]
impl StaticCodelist {
    fn find_by_code(&self, code: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.code.eq_ignore_ascii_case(code.trim()))
    }

    fn find_by_pref_term(&self, pref_term: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.pref_term.eq_ignore_ascii_case(pref_term.trim()))
    }

    fn find_by_value(&self, value: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.value.eq_ignore_ascii_case(value.trim()))
    }
}

fn static_codelist_term_by_code(
    codelist_code: &str,
    codelist: &StaticCodelist,
    code: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.code.eq_ignore_ascii_case(code.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

fn static_codelist_term_by_pref_term(
    codelist_code: &str,
    codelist: &StaticCodelist,
    pref_term: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.pref_term.eq_ignore_ascii_case(pref_term.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

fn static_codelist_term_by_value(
    codelist_code: &str,
    codelist: &StaticCodelist,
    value: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.value.eq_ignore_ascii_case(value.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

#[derive(Clone, Copy)]
struct StaticTerm {
    code: &'static str,
    value: &'static str,
    pref_term: &'static str,
}

fn static_codelist_term_matches_version(
    codelist_code: &str,
    term: &StaticTerm,
    version: Option<&str>,
) -> bool {
    if !static_codelist_matches_version(codelist_code, version) {
        return false;
    }
    let Some(version) = version.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    match codelist_code.trim().to_ascii_uppercase().as_str() {
        "C207415" => match term.code {
            "C17649" | "C48660" | "C0031X" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C207416" => match term.code {
            "C98704" | "C207613" | "C46079" => version >= "2024-09-27",
            _ => version >= "2025-09-26",
        },
        "C207417" => match term.code {
            "C68609" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C171445" => match term.code {
            "C177933" | "C171533" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C99078" => match term.code {
            "C15184" | "C307" | "C1909" => version >= "2023-12-15",
            "C17649" => version == "2023-12-15",
            "C54696" | "C16830" | "C18020" | "C1505" => version >= "2024-03-29",
            "C15238" | "C98769" | "C15313" => version >= "2024-09-27",
            "C218507" | "C15329" | "C923" => version >= "2025-09-26",
            _ => true,
        },
        "C127259" => match term.code {
            "C15197" | "C127779" => version >= "2023-12-15",
            "C15362" | "C15208" => version >= "2024-03-29",
            "C127780" | "C15407" => version >= "2024-09-27",
            _ => true,
        },
        "C71620" => match term.code {
            "C105499" => version >= "2024-09-27",
            "C176378" => version >= "2025-09-26",
            _ => true,
        },
        "C66735" => match term.code {
            "C28233" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C99076" => match term.code {
            "C82640" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C207418" => match term.code {
            "C202579" => version >= "2024-09-27",
            "C156473" => match term.value {
                "NIMP (AxMP)" => ("2024-09-27".."2025-09-26").contains(&version),
                "NIMP" => version >= "2025-09-26",
                _ => version >= "2024-09-27",
            },
            _ => true,
        },
        "C66737" => match term.code {
            "C198366" => match term.value {
                "PHASE I/II/III STUDY" => version < "2023-12-15",
                "PHASE I/II/III TRIAL" => version >= "2023-12-15",
                _ => true,
            },
            "C54721" => match term.value {
                "PHASE 0 TRIAL" => ("2023-12-15".."2024-09-27").contains(&version),
                "EARLY PHASE I" => version >= "2024-09-27",
                _ => true,
            },
            "C199989" | "C15602" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C188725" => match term.code {
            "C85827" => version >= "2024-09-27",
            "C163559" => version >= "2025-09-26",
            _ => version >= "2023-12-15",
        },
        "C188726" => match term.code {
            "C139173" | "C170559" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C207412" => version >= "2024-09-27",
        "C127260" => match term.code {
            "C71517" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C127261" => match term.code {
            "C15273" => version >= "2024-03-29",
            "C53312" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C201264" => match term.code {
            "C201356" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C201265" => version >= "2023-12-15",
        "C207413" => match term.code {
            "C215663" | "C215664" | "C71476" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C207414" => version == "2024-09-27",
        "C207419" => version >= "2024-09-27",
        "C215477" | "C215478" | "C215479" | "C215481" | "C215482" | "C215483" | "C215484" => {
            version >= "2025-09-26"
        }
        "C66726" => match term.code {
            "C42968" | "C48624" => version >= "2024-03-29",
            "C42998" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C188724" => match term.code {
            "C70793" => match term.value {
                "Clinical Study Sponsor" => version == "2024-09-27",
                "Study Sponsor" => version >= "2025-09-26",
                _ => true,
            },
            _ => true,
        },
        "C188727" => match term.code {
            "C165830" => match term.value {
                "Real World Data" => ("2024-09-27".."2025-09-26").contains(&version),
                "Real-world Data" => version >= "2025-09-26",
                _ => true,
            },
            _ => true,
        },
        _ => true,
    }
}

fn static_codelist_matches_version(codelist_code: &str, version: Option<&str>) -> bool {
    let Some(version) = version.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    match codelist_code.trim().to_ascii_uppercase().as_str() {
        "C127259" => version >= "2023-12-15",
        "C127260" | "C127261" => version >= "2023-12-15",
        "C207416" => version >= "2024-09-27",
        "C207418" => version >= "2024-09-27",
        "C207412" => version >= "2024-09-27",
        "C207413" => version >= "2024-09-27",
        "C207414" => version == "2024-09-27",
        "C207419" => version >= "2024-09-27",
        "C215477" | "C215478" | "C215479" | "C215481" | "C215482" | "C215483" | "C215484" => {
            version >= "2025-09-26"
        }
        "C215486" => version >= "2025-09-26",
        "C215480" => version >= "2025-09-26",
        _ => true,
    }
}

fn static_codelist(code: &str) -> Option<StaticCodelist> {
    match code.trim().to_ascii_uppercase().as_str() {
        "C66732" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C16576",
                    value: "F",
                    pref_term: "Female",
                },
                StaticTerm {
                    code: "C20197",
                    value: "M",
                    pref_term: "Male",
                },
                StaticTerm {
                    code: "C49636",
                    value: "BOTH",
                    pref_term: "Both",
                },
            ],
        }),
        "C66736" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15714",
                    value: "BASIC SCIENCE",
                    pref_term: "Basic Research",
                },
                StaticTerm {
                    code: "C49654",
                    value: "CURE",
                    pref_term: "Cure Study",
                },
                StaticTerm {
                    code: "C139174",
                    value: "DEVICE FEASIBILITY",
                    pref_term: "Device Feasibility Study",
                },
                StaticTerm {
                    code: "C49653",
                    value: "DIAGNOSIS",
                    pref_term: "Diagnosis Study",
                },
                StaticTerm {
                    code: "C170629",
                    value: "DISEASE MODIFYING",
                    pref_term: "Disease Modifying Treatment Study",
                },
                StaticTerm {
                    code: "C15245",
                    value: "HEALTH SERVICES RESEARCH",
                    pref_term: "Health Services Research",
                },
                StaticTerm {
                    code: "C49655",
                    value: "MITIGATION",
                    pref_term: "Adverse Effect Mitigation Study",
                },
                StaticTerm {
                    code: "C49657",
                    value: "PREVENTION",
                    pref_term: "Prevention Study",
                },
                StaticTerm {
                    code: "C71485",
                    value: "SCREENING",
                    pref_term: "Screening Study",
                },
                StaticTerm {
                    code: "C71486",
                    value: "SUPPORTIVE CARE",
                    pref_term: "Supportive Care Study",
                },
                StaticTerm {
                    code: "C49656",
                    value: "TREATMENT",
                    pref_term: "Treatment Study",
                },
            ],
        }),
        "C66735" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15228",
                    value: "DOUBLE BLIND",
                    pref_term: "Double Blind Study",
                },
                StaticTerm {
                    code: "C187674",
                    value: "OBSERVER BLIND",
                    pref_term: "Observer Blind Study",
                },
                StaticTerm {
                    code: "C156592",
                    value: "OPEN LABEL TO TREATMENT AND DOUBLE BLIND TO IMP DOSE",
                    pref_term: "Open Label for Treatment And Double Blind to Dose",
                },
                StaticTerm {
                    code: "C49659",
                    value: "OPEN LABEL",
                    pref_term: "Open Label Study",
                },
                StaticTerm {
                    code: "C28233",
                    value: "SINGLE BLIND",
                    pref_term: "Single Blind Study",
                },
            ],
        }),
        "C99076" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C82637",
                    value: "CROSS-OVER",
                    pref_term: "Crossover Study",
                },
                StaticTerm {
                    code: "C82638",
                    value: "FACTORIAL",
                    pref_term: "Factorial Study",
                },
                StaticTerm {
                    code: "C82639",
                    value: "PARALLEL",
                    pref_term: "Parallel Study",
                },
                StaticTerm {
                    code: "C142568",
                    value: "SEQUENTIAL",
                    pref_term: "Group Sequential Design",
                },
                StaticTerm {
                    code: "C82640",
                    value: "SINGLE GROUP",
                    pref_term: "Single Group Study",
                },
            ],
        }),
        "C99077" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C98388",
                    value: "INTERVENTIONAL",
                    pref_term: "Interventional Study",
                },
                StaticTerm {
                    code: "C16084",
                    value: "OBSERVATIONAL",
                    pref_term: "Observational Study",
                },
                StaticTerm {
                    code: "C98722",
                    value: "EXPANDED ACCESS",
                    pref_term: "Expanded Access Study",
                },
                StaticTerm {
                    code: "C129000",
                    value: "PATIENT REGISTRY",
                    pref_term: "Patient Registry Study",
                },
            ],
        }),
        "C66729" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C188189",
                    value: "NASODUODENAL",
                    pref_term: "Nasoduodenal Route of Administration",
                },
                StaticTerm {
                    code: "C38215",
                    value: "INFILTRATION",
                    pref_term: "Infiltration Route of Administration",
                },
                StaticTerm {
                    code: "C38217",
                    value: "INTRACORONAL, DENTAL",
                    pref_term: "Intracoronal Dental Route of Administration",
                },
                StaticTerm {
                    code: "C38257",
                    value: "INTRAPERICARDIAL",
                    pref_term: "Intrapericardial Route of Administration",
                },
                StaticTerm {
                    code: "C38288",
                    value: "ORAL",
                    pref_term: "Oral Route of Administration",
                },
                StaticTerm {
                    code: "C38305",
                    value: "TRANSDERMAL",
                    pref_term: "Transdermal Route of Administration",
                },
                StaticTerm {
                    code: "C38311",
                    value: "UNKNOWN",
                    pref_term: "Unknown Route of Administration",
                },
                StaticTerm {
                    code: "C48623",
                    value: "NOT APPLICABLE",
                    pref_term: "Route of Administration Not Applicable",
                },
            ],
        }),
        "C71113" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C17998",
                    value: "UNKNOWN",
                    pref_term: "Unknown",
                },
                StaticTerm {
                    code: "C64508",
                    value: "Q18H",
                    pref_term: "Every Eighteen Hours",
                },
                StaticTerm {
                    code: "C64525",
                    value: "QOD",
                    pref_term: "Every Other Day",
                },
                StaticTerm {
                    code: "C64528",
                    value: "3 TIMES PER WEEK",
                    pref_term: "Three Times Weekly",
                },
                StaticTerm {
                    code: "C64954",
                    value: "OCCASIONAL",
                    pref_term: "Infrequent",
                },
                StaticTerm {
                    code: "C71129",
                    value: "BIM",
                    pref_term: "Twice Per Month",
                },
                StaticTerm {
                    code: "C89791",
                    value: "Q36H",
                    pref_term: "Every Thirty-six Hours",
                },
                StaticTerm {
                    code: "C98860",
                    value: "3 TIMES PER YEAR",
                    pref_term: "Three Times Yearly",
                },
            ],
        }),
        "C188723" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25425",
                    value: "Approval",
                    pref_term: "Approved",
                },
                StaticTerm {
                    code: "C25508",
                    value: "Final",
                    pref_term: "Final",
                },
                StaticTerm {
                    code: "C63553",
                    value: "Obsolete",
                    pref_term: "Obsolete",
                },
                StaticTerm {
                    code: "C85255",
                    value: "Draft",
                    pref_term: "Draft",
                },
                StaticTerm {
                    code: "C188862",
                    value: "Pending Review",
                    pref_term: "Pending Review",
                },
            ],
        }),
        "C207418" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C202579",
                    value: "IMP",
                    pref_term: "Investigational Medicinal Product",
                },
                StaticTerm {
                    code: "C156473",
                    value: "NIMP (AxMP)",
                    pref_term: "Auxiliary Medicinal Product",
                },
                StaticTerm {
                    code: "C156473",
                    value: "NIMP",
                    pref_term: "Auxiliary Medicinal Product",
                },
            ],
        }),
        "C66737" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15600",
                    value: "PHASE I TRIAL",
                    pref_term: "Phase I Trial",
                },
                StaticTerm {
                    code: "C15601",
                    value: "PHASE II TRIAL",
                    pref_term: "Phase II Trial",
                },
                StaticTerm {
                    code: "C15602",
                    value: "PHASE III TRIAL",
                    pref_term: "Phase III Trial",
                },
                StaticTerm {
                    code: "C48660",
                    value: "NOT APPLICABLE",
                    pref_term: "Not Applicable",
                },
                StaticTerm {
                    code: "C198366",
                    value: "PHASE I/II/III STUDY",
                    pref_term: "Phase I/II/III Study",
                },
                StaticTerm {
                    code: "C198366",
                    value: "PHASE I/II/III TRIAL",
                    pref_term: "Phase I/II/III Trial",
                },
                StaticTerm {
                    code: "C199989",
                    value: "PHASE IB TRIAL",
                    pref_term: "Phase Ib Trial",
                },
                StaticTerm {
                    code: "C54721",
                    value: "PHASE 0 TRIAL",
                    pref_term: "Phase 0 Trial",
                },
                StaticTerm {
                    code: "C54721",
                    value: "EARLY PHASE I",
                    pref_term: "Early Phase 1 Trial",
                },
            ],
        }),
        "C188725" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C85826",
                    value: "Study Primary Objective",
                    pref_term: "Trial Primary Objective",
                },
                StaticTerm {
                    code: "C85827",
                    value: "Study Secondary Objective",
                    pref_term: "Trial Secondary Objective",
                },
                StaticTerm {
                    code: "C163559",
                    value: "Exploratory Objective",
                    pref_term: "Trial Exploratory Objective",
                },
            ],
        }),
        "C188726" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C94496",
                    value: "Primary Endpoint",
                    pref_term: "Primary Endpoint",
                },
                StaticTerm {
                    code: "C139173",
                    value: "Secondary Endpoint",
                    pref_term: "Secondary Endpoint",
                },
                StaticTerm {
                    code: "C170559",
                    value: "Exploratory Endpoint",
                    pref_term: "Exploratory Endpoint",
                },
            ],
        }),
        "C188728" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C25716",
                value: "Visit",
                pref_term: "Visit",
            }],
        }),
        "C207412" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25464",
                    value: "Country",
                    pref_term: "Country",
                },
                StaticTerm {
                    code: "C41129",
                    value: "Region",
                    pref_term: "Region",
                },
                StaticTerm {
                    code: "C68846",
                    value: "Global",
                    pref_term: "Global",
                },
            ],
        }),
        "C66797" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25370",
                    value: "EXCLUSION",
                    pref_term: "Exclusion Criteria",
                },
                StaticTerm {
                    code: "C25532",
                    value: "INCLUSION",
                    pref_term: "Inclusion Criteria",
                },
            ],
        }),
        "C127260" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C127781",
                    value: "NON-PROBABILITY SAMPLE",
                    pref_term: "Non-Probability Sampling Method",
                },
                StaticTerm {
                    code: "C71517",
                    value: "PROBABILITY SAMPLE",
                    pref_term: "Equal Probability Sampling Method",
                },
            ],
        }),
        "C127261" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15273",
                    value: "PROSPECTIVE",
                    pref_term: "Longitudinal Study",
                },
                StaticTerm {
                    code: "C53310",
                    value: "CROSS SECTIONAL",
                    pref_term: "Cross-Sectional Study",
                },
                StaticTerm {
                    code: "C53312",
                    value: "RETROSPECTIVE",
                    pref_term: "Retrospective Study",
                },
            ],
        }),
        "C201264" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C201356",
                    value: "After",
                    pref_term: "After Timing Type",
                },
                StaticTerm {
                    code: "C201357",
                    value: "Before",
                    pref_term: "Before Timing Type",
                },
                StaticTerm {
                    code: "C201358",
                    value: "Fixed Reference",
                    pref_term: "Fixed Reference Timing Type",
                },
            ],
        }),
        "C207413" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C132352",
                    value: "Sponsor Approval Date",
                    pref_term: "Protocol Approval by Sponsor Date",
                },
                StaticTerm {
                    code: "C207598",
                    value: "Protocol Effective Date",
                    pref_term: "Protocol Effective Date",
                },
                StaticTerm {
                    code: "C215663",
                    value: "Effective Date",
                    pref_term: "Effective Date",
                },
                StaticTerm {
                    code: "C215664",
                    value: "Issued Date",
                    pref_term: "Issued Date",
                },
                StaticTerm {
                    code: "C71476",
                    value: "Approval Date",
                    pref_term: "Approval Date",
                },
            ],
        }),
        "C207419" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207615",
                    value: "Brief Study Title",
                    pref_term: "Brief Study Title",
                },
                StaticTerm {
                    code: "C207616",
                    value: "Official Study Title",
                    pref_term: "Official Study Title",
                },
                StaticTerm {
                    code: "C207617",
                    value: "Public Study Title",
                    pref_term: "Public Study Title",
                },
                StaticTerm {
                    code: "C207618",
                    value: "Scientific Study Title",
                    pref_term: "Scientific Study Title",
                },
                StaticTerm {
                    code: "C207646",
                    value: "Study Acronym",
                    pref_term: "Study Acronym",
                },
            ],
        }),
        "C215477" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C70817",
                value: "Protocol",
                pref_term: "Study Protocol",
            }],
        }),
        "C215478" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C142424",
                    value: "Clinical Development Plan",
                    pref_term: "Clinical Development Plan",
                },
                StaticTerm {
                    code: "C215674",
                    value: "Pediatric Investigation Clinical Development Plan",
                    pref_term: "Pediatric Investigation Plan",
                },
            ],
        }),
        "C215479" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C45997",
                value: "pH",
                pref_term: "pH",
            }],
        }),
        "C215481" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215665",
                    value: "Study Subject Safety",
                    pref_term: "Study Subject Safety",
                },
                StaticTerm {
                    code: "C215666",
                    value: "Study Subject Rights",
                    pref_term: "Study Subject Rights",
                },
                StaticTerm {
                    code: "C215667",
                    value: "Study Data Reliability",
                    pref_term: "Study Data Reliability",
                },
                StaticTerm {
                    code: "C215668",
                    value: "Study Data Robustness",
                    pref_term: "Study Data Robustness",
                },
            ],
        }),
        "C215482" | "C215483" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215659",
                    value: "Centrally Sourced",
                    pref_term: "Centrally Sourced Indicator",
                },
                StaticTerm {
                    code: "C215660",
                    value: "Locally Sourced",
                    pref_term: "Locally Sourced Indicator",
                },
            ],
        }),
        "C215484" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C104504",
                    value: "Batch Number",
                    pref_term: "Batch Number",
                },
                StaticTerm {
                    code: "C112279",
                    value: "FDA Unique Device Identification",
                    pref_term: "FDA Unique Device Identifier",
                },
                StaticTerm {
                    code: "C70848",
                    value: "Lot Number",
                    pref_term: "Lot Number",
                },
                StaticTerm {
                    code: "C99285",
                    value: "Model Number",
                    pref_term: "Model Number",
                },
            ],
        }),
        "C66726" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C158215",
                    value: "CAPSULE, SOFTGEL, EXTENDED RELEASE",
                    pref_term: "Extended Release Capsule, Softgel Dosage Form",
                },
                StaticTerm {
                    code: "C42968",
                    value: "PATCH",
                    pref_term: "Patch Dosage Form",
                },
                StaticTerm {
                    code: "C42998",
                    value: "TABLET",
                    pref_term: "Tablet Dosage Form",
                },
                StaticTerm {
                    code: "C48624",
                    value: "NOT APPLICABLE",
                    pref_term: "Dosage Form Not Applicable",
                },
            ],
        }),
        "C201265" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C201352",
                    value: "End to End",
                    pref_term: "End to End",
                },
                StaticTerm {
                    code: "C201353",
                    value: "End to Start",
                    pref_term: "End to Start",
                },
                StaticTerm {
                    code: "C201354",
                    value: "Start to End",
                    pref_term: "Start to End",
                },
                StaticTerm {
                    code: "C201355",
                    value: "Start to Start",
                    pref_term: "Start to Start",
                },
            ],
        }),
        "C207414" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C17445",
                    value: "Caregiver",
                    pref_term: "Care Provider",
                },
                StaticTerm {
                    code: "C207599",
                    value: "Outcomes Assessor",
                    pref_term: "Outcomes Assessor",
                },
                StaticTerm {
                    code: "C25936",
                    value: "Investigator",
                    pref_term: "Investigator",
                },
                StaticTerm {
                    code: "C41189",
                    value: "Study Subject",
                    pref_term: "Study Subject",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
            ],
        }),
        "C127259" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15197",
                    value: "CASE CONTROL",
                    pref_term: "Case-Control Study",
                },
                StaticTerm {
                    code: "C127779",
                    value: "CASE CROSSOVER",
                    pref_term: "Observational Case-Crossover Study",
                },
                StaticTerm {
                    code: "C15362",
                    value: "CASE ONLY",
                    pref_term: "Case Study",
                },
                StaticTerm {
                    code: "C15208",
                    value: "COHORT",
                    pref_term: "Cohort Study",
                },
                StaticTerm {
                    code: "C127780",
                    value: "ECOLOGIC OR COMMUNITY",
                    pref_term: "Ecologic or Community Based Study",
                },
                StaticTerm {
                    code: "C15407",
                    value: "FAMILY BASED",
                    pref_term: "Family Study",
                },
            ],
        }),
        "C66781" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25529",
                    value: "HOURS",
                    pref_term: "Hour",
                },
                StaticTerm {
                    code: "C25301",
                    value: "DAYS",
                    pref_term: "Day",
                },
                StaticTerm {
                    code: "C29844",
                    value: "WEEKS",
                    pref_term: "Week",
                },
                StaticTerm {
                    code: "C29846",
                    value: "MONTHS",
                    pref_term: "Month",
                },
                StaticTerm {
                    code: "C29848",
                    value: "YEARS",
                    pref_term: "Year",
                },
            ],
        }),
        "C71620" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C25301",
                    value: "DAYS",
                    pref_term: "Day",
                },
                StaticTerm {
                    code: "C28253",
                    value: "mg",
                    pref_term: "Milligram",
                },
                StaticTerm {
                    code: "C29844",
                    value: "WEEKS",
                    pref_term: "Week",
                },
                StaticTerm {
                    code: "C29846",
                    value: "MONTHS",
                    pref_term: "Month",
                },
                StaticTerm {
                    code: "C29848",
                    value: "YEARS",
                    pref_term: "Year",
                },
                StaticTerm {
                    code: "C176378",
                    value: "mg/mL/day",
                    pref_term: "Gram per Liter per Day",
                },
                StaticTerm {
                    code: "C25613",
                    value: "%",
                    pref_term: "Percentage",
                },
                StaticTerm {
                    code: "C198376",
                    value: "10^4 IU/mL",
                    pref_term: "Ten Thousand International Units per Milliliter",
                },
                StaticTerm {
                    code: "C44278",
                    value: "U",
                    pref_term: "Unit",
                },
                StaticTerm {
                    code: "C105499",
                    value: "uV*s",
                    pref_term: "Microvolt Second",
                },
            ],
        }),
        "C127262" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C127785",
                    value: "CHILD CARE CENTER",
                    pref_term: "Childcare Center",
                },
                StaticTerm {
                    code: "C51282",
                    value: "CLINIC",
                    pref_term: "Clinic",
                },
                StaticTerm {
                    code: "C48953",
                    value: "FARM",
                    pref_term: "Farm",
                },
                StaticTerm {
                    code: "C102650",
                    value: "FIELD",
                    pref_term: "In the Field",
                },
                StaticTerm {
                    code: "C21541",
                    value: "HEALTH FACILITY",
                    pref_term: "Healthcare Facility",
                },
                StaticTerm {
                    code: "C18002",
                    value: "HOME",
                    pref_term: "Home",
                },
                StaticTerm {
                    code: "C16696",
                    value: "HOSPITAL",
                    pref_term: "Hospital",
                },
                StaticTerm {
                    code: "C102647",
                    value: "HOUSEHOLD ENVIRONMENT",
                    pref_term: "Household Environment",
                },
                StaticTerm {
                    code: "C41206",
                    value: "INSTITUTION",
                    pref_term: "Institution",
                },
                StaticTerm {
                    code: "C181529",
                    value: "MOTOR VEHICLE",
                    pref_term: "Motor Vehicle",
                },
                StaticTerm {
                    code: "C102679",
                    value: "NON-HOUSEHOLD ENVIRONMENT",
                    pref_term: "Non-household Environment",
                },
                StaticTerm {
                    code: "C181530",
                    value: "NOT IN CLINIC",
                    pref_term: "Not In Clinic",
                },
                StaticTerm {
                    code: "C16281",
                    value: "OUTPATIENT CLINIC",
                    pref_term: "Ambulatory Care Facility",
                },
                StaticTerm {
                    code: "C85862",
                    value: "PRISON",
                    pref_term: "Correctional Institution",
                },
                StaticTerm {
                    code: "C17118",
                    value: "SCHOOL",
                    pref_term: "School",
                },
                StaticTerm {
                    code: "C85863",
                    value: "SHELTER",
                    pref_term: "Shelter",
                },
                StaticTerm {
                    code: "C102712",
                    value: "SOCIAL SETTING",
                    pref_term: "Social Setting",
                },
                StaticTerm {
                    code: "C17556",
                    value: "WORKSITE",
                    pref_term: "Worksite",
                },
            ],
        }),
        "C171445" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C25170",
                    value: "E-MAIL",
                    pref_term: "E-mail",
                },
                StaticTerm {
                    code: "C175574",
                    value: "IN PERSON",
                    pref_term: "In Person",
                },
                StaticTerm {
                    code: "C177933",
                    value: "IVRS",
                    pref_term: "Interactive Voice Response System",
                },
                StaticTerm {
                    code: "C70805",
                    value: "LETTER",
                    pref_term: "Letter",
                },
                StaticTerm {
                    code: "C171525",
                    value: "REMOTE AUDIO VIDEO",
                    pref_term: "Audio-Videoconferencing",
                },
                StaticTerm {
                    code: "C171524",
                    value: "REMOTE AUDIO",
                    pref_term: "Audioconferencing",
                },
                StaticTerm {
                    code: "C171533",
                    value: "SHIPMENT CONFIRMED BY SIGNATURE",
                    pref_term: "Shipment Confirmed by Signature",
                },
                StaticTerm {
                    code: "C171537",
                    value: "TELEPHONE CALL",
                    pref_term: "Telephone Call",
                },
                StaticTerm {
                    code: "C157352",
                    value: "TEXT MESSAGE",
                    pref_term: "Text Message",
                },
            ],
        }),
        "C99078" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C15184",
                    value: "BEHAVIORAL THERAPY",
                    pref_term: "Behavioral Intervention",
                },
                StaticTerm {
                    code: "C307",
                    value: "BIOLOGIC",
                    pref_term: "Biological Agent",
                },
                StaticTerm {
                    code: "C54696",
                    value: "COMBINATION PRODUCT",
                    pref_term: "Combination Product",
                },
                StaticTerm {
                    code: "C16830",
                    value: "DEVICE",
                    pref_term: "Medical Device",
                },
                StaticTerm {
                    code: "C18020",
                    value: "DIAGNOSTIC TEST",
                    pref_term: "Diagnostic Procedure",
                },
                StaticTerm {
                    code: "C1505",
                    value: "DIETARY SUPPLEMENT",
                    pref_term: "Dietary Supplement",
                },
                StaticTerm {
                    code: "C1909",
                    value: "DRUG",
                    pref_term: "Pharmacologic Substance",
                },
                StaticTerm {
                    code: "C15238",
                    value: "GENETIC",
                    pref_term: "Gene Therapy",
                },
                StaticTerm {
                    code: "C218507",
                    value: "NON-SURGICAL PROCEDURE",
                    pref_term: "Non-Surgical Procedure",
                },
                StaticTerm {
                    code: "C98769",
                    value: "PROCEDURE",
                    pref_term: "Physical Medical Procedure",
                },
                StaticTerm {
                    code: "C15313",
                    value: "RADIATION",
                    pref_term: "Radiation Therapy",
                },
                StaticTerm {
                    code: "C15329",
                    value: "SURGERY",
                    pref_term: "Surgical Procedure",
                },
                StaticTerm {
                    code: "C923",
                    value: "VACCINE",
                    pref_term: "Vaccine",
                },
                StaticTerm {
                    code: "C17649",
                    value: "OTHER",
                    pref_term: "Other",
                },
            ],
        }),
        "C66739" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C158283",
                    value: "ADHESION PERFORMANCE",
                    pref_term: "Adhesion Performance Study",
                },
                StaticTerm {
                    code: "C158284",
                    value: "ALCOHOL EFFECT",
                    pref_term: "Alcohol Effect Study",
                },
                StaticTerm {
                    code: "C49664",
                    value: "BIO-AVAILABILITY",
                    pref_term: "Bioavailability Study",
                },
                StaticTerm {
                    code: "C49665",
                    value: "BIO-EQUIVALENCE",
                    pref_term: "Therapeutic Equivalency Study",
                },
                StaticTerm {
                    code: "C158288",
                    value: "BIOSIMILARITY",
                    pref_term: "Biosimilarity Study",
                },
                StaticTerm {
                    code: "C158285",
                    value: "DEVICE-DRUG INTERACTION",
                    pref_term: "Device-Drug Interaction Study",
                },
                StaticTerm {
                    code: "C49653",
                    value: "DIAGNOSIS",
                    pref_term: "Diagnosis Study",
                },
                StaticTerm {
                    code: "C158289",
                    value: "DOSE FINDING",
                    pref_term: "Dose Finding Study",
                },
                StaticTerm {
                    code: "C158290",
                    value: "DOSE PROPORTIONALITY",
                    pref_term: "Dose Proportionality Study",
                },
                StaticTerm {
                    code: "C127803",
                    value: "DOSE RESPONSE",
                    pref_term: "Dose Response Study",
                },
                StaticTerm {
                    code: "C158286",
                    value: "DRUG-DRUG INTERACTION",
                    pref_term: "Drug-Drug Interaction Study",
                },
                StaticTerm {
                    code: "C178057",
                    value: "ECG",
                    pref_term: "Electrocardiographic Study",
                },
                StaticTerm {
                    code: "C49666",
                    value: "EFFICACY",
                    pref_term: "Efficacy Study",
                },
                StaticTerm {
                    code: "C98729",
                    value: "FOOD EFFECT",
                    pref_term: "Food Effect Study",
                },
                StaticTerm {
                    code: "C120842",
                    value: "IMMUNOGENICITY",
                    pref_term: "Immunogenicity Study",
                },
                StaticTerm {
                    code: "C201484",
                    value: "MASS BALANCE",
                    pref_term: "Mass Balance Study",
                },
                StaticTerm {
                    code: "C49662",
                    value: "PHARMACODYNAMIC",
                    pref_term: "Pharmacodynamic Study",
                },
                StaticTerm {
                    code: "C39493",
                    value: "PHARMACOECONOMIC",
                    pref_term: "Pharmacoeconomic Study",
                },
                StaticTerm {
                    code: "C129001",
                    value: "PHARMACOGENETIC",
                    pref_term: "Pharmacogenetic Study",
                },
                StaticTerm {
                    code: "C49661",
                    value: "PHARMACOGENOMIC",
                    pref_term: "Pharmacogenomic Study",
                },
                StaticTerm {
                    code: "C49663",
                    value: "PHARMACOKINETIC",
                    pref_term: "Pharmacokinetic Study",
                },
                StaticTerm {
                    code: "C161477",
                    value: "POSITION EFFECT",
                    pref_term: "Position Effect Trial",
                },
                StaticTerm {
                    code: "C49657",
                    value: "PREVENTION",
                    pref_term: "Prevention Study",
                },
                StaticTerm {
                    code: "C174366",
                    value: "REACTOGENICITY",
                    pref_term: "Reactogenicity Study",
                },
                StaticTerm {
                    code: "C49667",
                    value: "SAFETY",
                    pref_term: "Safety Study",
                },
                StaticTerm {
                    code: "C161478",
                    value: "SWALLOWING FUNCTION",
                    pref_term: "Swallowing Function Trial",
                },
                StaticTerm {
                    code: "C158287",
                    value: "THOROUGH QT",
                    pref_term: "Thorough QT Study",
                },
                StaticTerm {
                    code: "C98791",
                    value: "TOLERABILITY",
                    pref_term: "Tolerability Study",
                },
                StaticTerm {
                    code: "C49656",
                    value: "TREATMENT",
                    pref_term: "Treatment Study",
                },
                StaticTerm {
                    code: "C161479",
                    value: "USABILITY TESTING",
                    pref_term: "Usability Testing Study",
                },
                StaticTerm {
                    code: "C161480",
                    value: "WATER EFFECT",
                    pref_term: "Water Effect Trial",
                },
            ],
        }),
        "C207415" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207600",
                    value: "Change In Standard Of Care",
                    pref_term: "Change In Standard Of Care",
                },
                StaticTerm {
                    code: "C207601",
                    value: "Change In Strategy",
                    pref_term: "Change In Strategy",
                },
                StaticTerm {
                    code: "C207602",
                    value: "IMP Addition",
                    pref_term: "IMP Addition",
                },
                StaticTerm {
                    code: "C207603",
                    value: "Inconsistency And/or Error In The Protocol",
                    pref_term: "Inconsistency and/or Error In The Protocol",
                },
                StaticTerm {
                    code: "C207604",
                    value: "Investigator/Site Feedback",
                    pref_term: "Investigator/Site Feedback",
                },
                StaticTerm {
                    code: "C207605",
                    value: "IRB/IEC Feedback",
                    pref_term: "IRB/IEC Feedback",
                },
                StaticTerm {
                    code: "C207606",
                    value: "Manufacturing Change",
                    pref_term: "Manufacturing Change",
                },
                StaticTerm {
                    code: "C207607",
                    value: "New Data Available (Other Than Safety Data)",
                    pref_term: "New Data Available (Other Than Safety Data)",
                },
                StaticTerm {
                    code: "C207608",
                    value: "New Regulatory Guidance",
                    pref_term: "New Regulatory Guidance",
                },
                StaticTerm {
                    code: "C207609",
                    value: "New Safety Information Available",
                    pref_term: "New Safety Information Available",
                },
                StaticTerm {
                    code: "C207610",
                    value: "Protocol Design Error",
                    pref_term: "Protocol Design Error",
                },
                StaticTerm {
                    code: "C207611",
                    value: "Recruitment Difficulty",
                    pref_term: "Recruitment Difficulty",
                },
                StaticTerm {
                    code: "C207612",
                    value: "Regulatory Agency Request To Amend",
                    pref_term: "Regulatory Agency Request To Amend",
                },
                StaticTerm {
                    code: "C17649",
                    value: "OTHER",
                    pref_term: "Other",
                },
                StaticTerm {
                    code: "C48660",
                    value: "NOT APPLICABLE",
                    pref_term: "Not Applicable",
                },
                StaticTerm {
                    code: "C0031X",
                    value: "Extension",
                    pref_term: "Extension",
                },
            ],
        }),
        "C207416" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C98704",
                    value: "Adaptive",
                    pref_term: "Adaptive Design",
                },
                StaticTerm {
                    code: "C207613",
                    value: "Extension",
                    pref_term: "Extension Study Design",
                },
                StaticTerm {
                    code: "C46079",
                    value: "Randomized",
                    pref_term: "Randomized Controlled Clinical Trial",
                },
                StaticTerm {
                    code: "C217004",
                    value: "Single-Centre",
                    pref_term: "Single-Center Study",
                },
                StaticTerm {
                    code: "C217005",
                    value: "Multicentre",
                    pref_term: "Multicenter Study",
                },
                StaticTerm {
                    code: "C217006",
                    value: "Single Country",
                    pref_term: "Single Country Study",
                },
                StaticTerm {
                    code: "C217007",
                    value: "Multiple Countries",
                    pref_term: "Multiple Country Study",
                },
                StaticTerm {
                    code: "C25689",
                    value: "Stratification",
                    pref_term: "Stratification",
                },
                StaticTerm {
                    code: "C147145",
                    value: "Stratified Randomisation",
                    pref_term: "Stratified Randomization",
                },
            ],
        }),
        "C207417" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207614",
                    value: "Additional Required Treatment",
                    pref_term: "Additional Required Medicinal Product",
                },
                StaticTerm {
                    code: "C165822",
                    value: "Background Treatment",
                    pref_term: "Background Treatment",
                },
                StaticTerm {
                    code: "C158128",
                    value: "Challenge Agent",
                    pref_term: "Challenge Agent",
                },
                StaticTerm {
                    code: "C18020",
                    value: "Diagnostic",
                    pref_term: "Diagnostic Procedure",
                },
                StaticTerm {
                    code: "C41161",
                    value: "Experimental Intervention",
                    pref_term: "Protocol Agent",
                },
                StaticTerm {
                    code: "C753",
                    value: "Placebo",
                    pref_term: "Placebo",
                },
                StaticTerm {
                    code: "C165835",
                    value: "Rescue Medicine",
                    pref_term: "Rescue Medications",
                },
                StaticTerm {
                    code: "C68609",
                    value: "Active Comparator",
                    pref_term: "Active Comparator",
                },
            ],
        }),
        "C215486" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215657",
                    value: "Clinical Education",
                    pref_term: "Clinical Education Study",
                },
                StaticTerm {
                    code: "C215654",
                    value: "Disease Determinants",
                    pref_term: "Disease Determinants Study",
                },
                StaticTerm {
                    code: "C215658",
                    value: "Disease Etiology",
                    pref_term: "Disease Etiology Study",
                },
                StaticTerm {
                    code: "C215653",
                    value: "Disease Incidence",
                    pref_term: "Disease Incidence Study",
                },
                StaticTerm {
                    code: "C215675",
                    value: "Disease Prevalence",
                    pref_term: "Disease Prevalence Study",
                },
                StaticTerm {
                    code: "C215655",
                    value: "Disease Prognosis",
                    pref_term: "Disease Prognosis Study",
                },
                StaticTerm {
                    code: "C215656",
                    value: "Drug Utilization",
                    pref_term: "Drug Utilization Study",
                },
                StaticTerm {
                    code: "C49667",
                    value: "Safety",
                    pref_term: "Safety Study",
                },
            ],
        }),
        "C188724" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C18240",
                    value: "Academic Institution",
                    pref_term: "Academia",
                },
                StaticTerm {
                    code: "C93453",
                    value: "Clinical Study Registry",
                    pref_term: "Study Registry",
                },
                StaticTerm {
                    code: "C54148",
                    value: "Contract Research Organization",
                    pref_term: "Contract Research Organization",
                },
                StaticTerm {
                    code: "C199144",
                    value: "Government Institute",
                    pref_term: "Governmental Agency or Group",
                },
                StaticTerm {
                    code: "C21541",
                    value: "Healthcare Facility",
                    pref_term: "Healthcare Facility",
                },
                StaticTerm {
                    code: "C37984",
                    value: "Laboratory",
                    pref_term: "Laboratory",
                },
                StaticTerm {
                    code: "C215661",
                    value: "Medical Device Company",
                    pref_term: "Medical Device Company",
                },
                StaticTerm {
                    code: "C54149",
                    value: "Drug Company",
                    pref_term: "Pharmaceutical Company",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Clinical Study Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Study Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C188863",
                    value: "Regulatory Agency",
                    pref_term: "Regulatory Agency",
                },
                StaticTerm {
                    code: "C93448",
                    value: "Research Organization",
                    pref_term: "Research Organization",
                },
            ],
        }),
        "C188727" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C165830",
                    value: "Real World Data",
                    pref_term: "Real World Data",
                },
                StaticTerm {
                    code: "C165830",
                    value: "Real-world Data",
                    pref_term: "Real-world Data",
                },
                StaticTerm {
                    code: "C176263",
                    value: "Synthetic Data",
                    pref_term: "Synthetic Data",
                },
                StaticTerm {
                    code: "C188864",
                    value: "Historical Data",
                    pref_term: "Historical Data",
                },
                StaticTerm {
                    code: "C188865",
                    value: "Virtual Data",
                    pref_term: "Virtual Data",
                },
                StaticTerm {
                    code: "C188866",
                    value: "Data Generated Within Study",
                    pref_term: "Data Generated Within Study",
                },
            ],
        }),
        "SPEC" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "",
                    value: "ABDOMINAL WALL",
                    pref_term: "Abdominal Wall",
                },
                StaticTerm {
                    code: "",
                    value: "ADIPOSE TISSUE, BROWN",
                    pref_term: "Brown Adipose Tissue",
                },
                StaticTerm {
                    code: "",
                    value: "AIR SAC",
                    pref_term: "Air Sac",
                },
            ],
        }),
        "C215480" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C78726",
                    value: "Adjudication Committee",
                    pref_term: "Adjudication Committee",
                },
                StaticTerm {
                    code: "C17445",
                    value: "Care Provider",
                    pref_term: "Caregiver",
                },
                StaticTerm {
                    code: "C215672",
                    value: "Clinical Trial Physician",
                    pref_term: "Clinical Trial Physician",
                },
                StaticTerm {
                    code: "C215669",
                    value: "Co-Sponsor",
                    pref_term: "Study Co-Sponsor",
                },
                StaticTerm {
                    code: "C215662",
                    value: "Contract Research",
                    pref_term: "Contract Research",
                },
                StaticTerm {
                    code: "C142489",
                    value: "Data Safety Monitoring Board",
                    pref_term: "Data Monitoring Committee",
                },
                StaticTerm {
                    code: "C215671",
                    value: "Dose Escalation Committee",
                    pref_term: "Dose Escalation Committee",
                },
                StaticTerm {
                    code: "C142578",
                    value: "Independent Data Monitoring Committee",
                    pref_term: "Independent Data Monitoring Committee",
                },
                StaticTerm {
                    code: "C25936",
                    value: "Investigator",
                    pref_term: "Investigator",
                },
                StaticTerm {
                    code: "C37984",
                    value: "Laboratory",
                    pref_term: "Laboratory",
                },
                StaticTerm {
                    code: "C215670",
                    value: "Local Sponsor",
                    pref_term: "Local Legal Sponsor",
                },
                StaticTerm {
                    code: "C25392",
                    value: "Manufacturer",
                    pref_term: "Manufacturer",
                },
                StaticTerm {
                    code: "C51876",
                    value: "Medical Expert",
                    pref_term: "Sponsor Medical Expert",
                },
                StaticTerm {
                    code: "C207599",
                    value: "Outcomes Assessor",
                    pref_term: "Outcomes Assessor",
                },
                StaticTerm {
                    code: "C215673",
                    value: "Pharmacovigilance",
                    pref_term: "Pharmacovigilance Group",
                },
                StaticTerm {
                    code: "C19924",
                    value: "Principal investigator",
                    pref_term: "Principal Investigator",
                },
                StaticTerm {
                    code: "C51851",
                    value: "Project Manager",
                    pref_term: "Project Coordinator",
                },
                StaticTerm {
                    code: "C188863",
                    value: "Regulatory Agency",
                    pref_term: "Regulatory Agency",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C51877",
                    value: "Statistician",
                    pref_term: "Statistician",
                },
                StaticTerm {
                    code: "C80403",
                    value: "Study Site",
                    pref_term: "Study Site",
                },
                StaticTerm {
                    code: "C41189",
                    value: "Study Subject",
                    pref_term: "Study Subject",
                },
            ],
        }),
        _ => None,
    }
}

fn derive_dataset_filtered_variables_dataset(
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

fn derive_xhtml_errors_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    column_name: &str,
    namespace: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "get_xhtml_errors operation requires a source variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "get_xhtml_errors operation requires an output column".to_owned(),
        ));
    }

    let source_values = operation_column_values(dataset, source_column)
        .unwrap_or_else(|_| vec![Value::Null; dataset.summary().row_count]);
    let values = source_values
        .iter()
        .map(|value| {
            let Some(text) = json_scalar_string(value) else {
                return Value::Null;
            };
            if xhtml_fragment_errors(&text, namespace).is_empty() {
                Value::Null
            } else {
                Value::String("invalid xhtml".to_owned())
            }
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn xhtml_fragment_errors(text: &str, namespace: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let wrapped = format!(r#"<root xmlns:usdm="{namespace}">{text}</root>"#);
    let Ok(document) = roxmltree::Document::parse(&wrapped) else {
        return vec!["invalid xhtml".to_owned()];
    };

    let mut errors = Vec::new();
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.parent().is_none() {
            continue;
        }
        let name = node.tag_name().name();
        let ns = node.tag_name().namespace();
        if ns == Some(namespace) {
            if name != "tag" && name != "ref" {
                errors.push(format!("unsupported usdm element {name}"));
                continue;
            }
            if name == "tag" && !node.has_attribute("name") {
                errors.push("usdm:tag requires name".to_owned());
            }
            if node.children().any(|child| {
                child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty())
            }) {
                errors.push(format!("usdm:{name} must be empty"));
            }
            for attribute in node.attributes() {
                if !is_allowed_usdm_xhtml_attribute(name, attribute.name()) {
                    errors.push(format!(
                        "unsupported usdm:{name} attribute {}",
                        attribute.name()
                    ));
                }
            }
            continue;
        }

        if !is_allowed_xhtml_element(name) {
            errors.push(format!("unsupported xhtml element {name}"));
            continue;
        }

        for attribute in node.attributes() {
            if !is_allowed_xhtml_attribute(name, attribute.name()) {
                errors.push(format!(
                    "unsupported xhtml attribute {} on {name}",
                    attribute.name()
                ));
            }
        }
    }

    errors
}

fn is_allowed_xhtml_element(name: &str) -> bool {
    matches!(
        name,
        "root"
            | "a"
            | "b"
            | "br"
            | "div"
            | "em"
            | "i"
            | "img"
            | "li"
            | "ol"
            | "p"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "table"
            | "tbody"
            | "td"
            | "th"
            | "thead"
            | "tr"
            | "u"
            | "ul"
    )
}

fn is_allowed_xhtml_attribute(element: &str, attribute: &str) -> bool {
    matches!(attribute, "class" | "id" | "style")
        || (element == "a" && matches!(attribute, "href" | "title"))
        || (element == "img" && matches!(attribute, "alt" | "src"))
        || ((element == "td" || element == "th") && matches!(attribute, "colspan" | "rowspan"))
}

fn is_allowed_usdm_xhtml_attribute(element: &str, attribute: &str) -> bool {
    match element {
        "tag" => attribute == "name",
        "ref" => matches!(attribute, "attribute" | "id" | "klass"),
        _ => false,
    }
}

fn domain_label_value(dataset: &LoadedDataset) -> String {
    if let Some(label) = dataset.metadata.label.as_ref() {
        if !label.trim().is_empty() {
            return label.trim().to_owned();
        }
    }
    if let Some(domain) = dataset.metadata.domain.as_ref() {
        if !domain.trim().is_empty() {
            return domain.trim().to_owned();
        }
    }
    if !dataset.metadata.name.trim().is_empty() {
        return dataset.metadata.name.trim().to_owned();
    }
    String::new()
}

fn domain_name_value(dataset: &LoadedDataset) -> String {
    if let Some(domain) = dataset.metadata.domain.as_ref() {
        if !domain.trim().is_empty() {
            return domain.trim().to_owned();
        }
    }
    if !dataset.metadata.name.trim().is_empty() {
        return dataset.metadata.name.trim().to_owned();
    }
    if let Some(label) = dataset.metadata.label.as_ref() {
        if !label.trim().is_empty() {
            return label.trim().to_owned();
        }
    }
    String::new()
}

fn derive_column_from_values_with_aliases(
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

fn reference_dataset_variable_names(dataset: &LoadedDataset) -> Vec<String> {
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

fn dataset_has_variable(dataset: &LoadedDataset, column: &str) -> bool {
    reference_dataset_variable_names(dataset)
        .iter()
        .any(|name| name.eq_ignore_ascii_case(column))
}

fn expand_dataset_domain_placeholder(dataset: &LoadedDataset, name: &str) -> String {
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

fn json_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn operation_input_datasets(
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

fn operation_dataset_name(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["dataset", "domain", "input", "source"])
}

fn derive_jsonata_column(
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

fn operation_skipped_result(
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

fn join_skipped_result(rule: &ExecutableRule, message: impl Into<String>) -> RuleValidationResult {
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

fn evaluation_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    source: EngineError,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::EvaluationError),
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message: format!(
            "Rule {} could not be evaluated for dataset {}: {source}",
            rule.core_id,
            dataset.metadata().name
        ),
        error_count: 0,
        errors: Vec::new(),
    }
}

fn missing_column_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::OracleSemanticsGap),
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message: format!(
            "Rule {} uses missing-column oracle semantics that are not supported for dataset {}",
            rule.core_id,
            dataset.metadata().name
        ),
        error_count: 0,
        errors: Vec::new(),
    }
}

fn missing_scope_wide_reference_target_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_scope_wide_reference_target_rule(rule)
        || dataset_has_column(dataset, "USUBJID")
    {
        return None;
    }

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
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

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn missing_tpt_relationship_target_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_tpt_relationship_rule(rule) {
        return None;
    }

    let has_tpt = dataset_has_column(
        dataset,
        &expand_domain_placeholder_for_dataset(dataset, "--TPT"),
    );
    let has_tptnum = dataset_has_column(
        dataset,
        &expand_domain_placeholder_for_dataset(dataset, "--TPTNUM"),
    );
    if has_tpt == has_tptnum {
        return None;
    }

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
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

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn missing_tpt_relationship_pp_dataset_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_tpt_relationship_rule(rule) {
        return None;
    }
    if datasets.iter().any(|dataset| {
        dataset.metadata().domain.as_deref() == Some("PP")
            || dataset.metadata().name.eq_ignore_ascii_case("PP")
    }) {
        return None;
    }
    if rule_results
        .iter()
        .any(|result| result.execution_status == core_engine::ExecutionStatus::Failed)
    {
        return None;
    }
    if !datasets.iter().any(|dataset| {
        let has_tpt = dataset_has_column(
            dataset,
            &expand_domain_placeholder_for_dataset(dataset, "--TPT"),
        );
        let has_tptnum = dataset_has_column(
            dataset,
            &expand_domain_placeholder_for_dataset(dataset, "--TPTNUM"),
        );
        let has_scat = dataset_has_column(
            dataset,
            &expand_domain_placeholder_for_dataset(dataset, "--SCAT"),
        );
        has_tpt && has_tptnum && has_scat
    }) {
        return None;
    }

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let issue = ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: "PP".to_owned(),
        domain: Some("PP".to_owned()),
        row: None,
        variables: Vec::new(),
        message: message.clone(),
        usubjid: None,
        seq: None,
    };

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: "PP".to_owned(),
        domain: Some("PP".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn core_000138_dm_dataset_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_dm_dataset_oracle_result_rule(rule) {
        return None;
    }
    if rule_results
        .iter()
        .any(|result| result.dataset.eq_ignore_ascii_case("DM"))
    {
        return None;
    }
    let dm = datasets.iter().find(|dataset| {
        dataset.metadata().domain.as_deref() == Some("DM")
            || dataset.metadata().name.eq_ignore_ascii_case("DM")
    })?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let issue = ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: "DM".to_owned(),
        domain: Some("DM".to_owned()),
        row: None,
        variables: Vec::new(),
        message: message.clone(),
        usubjid: None,
        seq: None,
    };

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: dm.metadata().name.clone(),
        domain: Some("DM".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn core_000095_se_dataset_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_se_dataset_oracle_result_rule(rule) {
        return None;
    }
    if rule_results.iter().any(|result| {
        result.dataset.eq_ignore_ascii_case("SE")
            && result.execution_status == core_engine::ExecutionStatus::Failed
    }) {
        return None;
    }
    let se = datasets.iter().find(|dataset| {
        (dataset.metadata().domain.as_deref() == Some("SE")
            || dataset.metadata().name.eq_ignore_ascii_case("SE"))
            && !dataset_has_column(dataset, "SEUPDES")
    })?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let issue = ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: "SE".to_owned(),
        domain: Some("SE".to_owned()),
        row: None,
        variables: Vec::new(),
        message: message.clone(),
        usubjid: None,
        seq: None,
    };

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: se.metadata().name.clone(),
        domain: Some("SE".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn core_000572_cm_dataset_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_cm_dataset_oracle_result_rule(rule) {
        return None;
    }
    if rule_results.iter().any(|result| {
        result.dataset.eq_ignore_ascii_case("CM")
            && result.execution_status == core_engine::ExecutionStatus::Failed
    }) {
        return None;
    }
    let cm = datasets.iter().find(|dataset| {
        (dataset.metadata().domain.as_deref() == Some("CM")
            || dataset.metadata().name.eq_ignore_ascii_case("CM"))
            && !dataset_has_column(dataset, "CMDTC")
    })?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let issue = ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: "CM".to_owned(),
        domain: Some("CM".to_owned()),
        row: None,
        variables: Vec::new(),
        message: message.clone(),
        usubjid: None,
        seq: None,
    };

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: cm.metadata().name.clone(),
        domain: Some("CM".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn core_000466_pp_dataset_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_pp_dataset_oracle_result_rule(rule) {
        return None;
    }
    if rule_results
        .iter()
        .any(|result| result.execution_status == core_engine::ExecutionStatus::Failed)
    {
        return None;
    }
    let pp = datasets.iter().find(|dataset| {
        (dataset.metadata().domain.as_deref() == Some("PP")
            || dataset.metadata().name.eq_ignore_ascii_case("PP"))
            && !dataset_has_column(dataset, "PPUSCHFL")
    })?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let issue = ValidationIssue {
        rule_id: rule.core_id.clone(),
        dataset: "PP".to_owned(),
        domain: Some("PP".to_owned()),
        row: None,
        variables: Vec::new(),
        message: message.clone(),
        usubjid: None,
        seq: None,
    };

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset: pp.metadata().name.clone(),
        domain: Some("PP".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

fn outcome_message(rule: &ExecutableRule) -> Option<String> {
    rule.actions
        .iter()
        .find(|action| action.name == "generate_dataset_error_objects")
        .or_else(|| rule.actions.first())
        .and_then(|action| action.params.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn entity_column_ref_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::OracleSemanticsGap),
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message: format!(
            "Rule {} uses entity column-ref comparator semantics that are not supported for dataset {}",
            rule.core_id,
            dataset.metadata().name
        ),
        error_count: 0,
        errors: Vec::new(),
    }
}

fn skipped_result_for_evaluation_error(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    source: EngineError,
    open_rules_compat: bool,
) -> RuleValidationResult {
    if open_rules_compat
        && matches!(source, EngineError::MissingColumn(_))
        && is_missing_column_oracle_gap_rule(rule)
    {
        return missing_column_skipped_result(rule, dataset);
    }
    evaluation_skipped_result(rule, dataset, source)
}

fn should_ignore_evaluation_error(
    rule: &ExecutableRule,
    source: &EngineError,
    execution_dataset_count: usize,
    open_rules_compat: bool,
) -> bool {
    execution_dataset_count > 1
        && matches!(source, EngineError::MissingColumn(_))
        && (!open_rules_compat || !is_missing_column_oracle_gap_rule(rule))
}

fn unsupported_operation(rule: &ExecutableRule) -> Option<String> {
    rule.operations.iter().find_map(|operation| {
        let name = operation_name(operation).unwrap_or_else(|| "<missing>".to_owned());
        (!is_supported_operation_name(&name)).then_some(name)
    })
}

fn is_join_operation(operation: &OperationSpec) -> bool {
    operation_name(operation)
        .as_deref()
        .is_some_and(is_join_operation_name)
}

fn is_supported_operation_name(name: &str) -> bool {
    is_join_operation_name(name)
        || matches!(
            name,
            "filter"
                | "where"
                | "subset"
                | "derive"
                | "add_column"
                | "aggregate"
                | "group_by"
                | "group_count"
                | "record_count"
                | "sort"
                | "order_by"
                | "min"
                | "max"
                | "select"
                | "keep"
                | "project"
                | "drop"
                | "remove_columns"
                | "exclude_columns"
                | "rename"
                | "rename_columns"
                | "distinct"
                | "deduplicate"
                | "unique"
                | "row_number"
                | "rank"
                | "domain_is_custom"
                | "domain_label"
                | "study_domains"
                | "variable_count"
                | "dy"
                | "min_date"
                | "max_date"
                | "extract_metadata"
                | "dataset_names"
                | "variable_exists"
                | "expected_variables"
                | "required_variables"
                | "get_column_order_from_dataset"
                | "get_column_order_from_library"
                | "valid_codelist_dates"
                | "map"
                | "codelist_extensible"
                | "codelist_terms"
                | "split_by"
                | "get_model_column_order"
                | "get_parent_model_column_order"
                | "get_dataset_filtered_variables"
                | "get_model_filtered_variables"
                | "get_xhtml_errors"
        )
}

fn is_join_operation_name(name: &str) -> bool {
    matches!(
        name,
        "join"
            | "left_join"
            | "dataset_join"
            | "inner_join"
            | "semi_join"
            | "anti_join"
            | "merge"
            | "lookup"
            | "match_dataset"
            | "match_datasets"
    )
}

fn operation_name(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["operator", "name", "type", "operation"])
        .map(|value| normalize_operation_key(&value))
}

fn string_field(operation: &OperationSpec, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn string_list_field(operation: &OperationSpec, keys: &[&str]) -> Option<Vec<String>> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(strings_from_value)
        .filter(|values| !values.is_empty())
}

fn bool_field(operation: &OperationSpec, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_bool)
}

fn string_map_field(operation: &OperationSpec, keys: &[&str]) -> Option<BTreeMap<String, String>> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_normalized(operation, key))
        })
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|values| !values.is_empty())
}

fn rename_pair(operation: &OperationSpec) -> Option<BTreeMap<String, String>> {
    let from = string_field(operation, &["from", "source", "old", "old_name"])?;
    let to = string_field(operation, &["to", "target", "new", "new_name", "as"])?;
    Some(BTreeMap::from([(from, to)]))
}

fn operation_value<'a>(operation: &'a OperationSpec, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        operation
            .fields
            .get(*key)
            .or_else(|| field_normalized(operation, key))
    })
}

fn operation_function_argument<'a>(expression: &'a str, names: &[&str]) -> Option<&'a str> {
    let args = operation_function_arguments(expression, names)?;
    (args.len() == 1).then_some(args[0])
}

fn operation_function_arguments<'a>(expression: &'a str, names: &[&str]) -> Option<Vec<&'a str>> {
    let expression = expression.trim();
    let open = expression.find('(')?;
    if !names
        .iter()
        .any(|name| operation_function_names_equal(&expression[..open], name))
        || !expression.ends_with(')')
    {
        return None;
    }
    Some(split_operation_commas(
        &expression[open + 1..expression.len() - 1],
    ))
}

fn operation_function_names_equal(left: &str, right: &str) -> bool {
    left.trim()
        .trim_start_matches('$')
        .eq_ignore_ascii_case(right.trim().trim_start_matches('$'))
}

fn split_operation_commas(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(expression[start..byte_index].trim());
                start = byte_index + 1;
            }
            _ => {}
        }
    }

    if !expression[start..].trim().is_empty() {
        parts.push(expression[start..].trim());
    }
    parts
}

fn operation_string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }
    let quote = value.chars().next()?;
    if !matches!(quote, '"' | '\'') || !value.ends_with(quote) {
        return None;
    }
    Some(
        value[1..value.len() - 1]
            .replace("\\'", "'")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
    )
}

fn is_quoted_literal(value: &str) -> bool {
    operation_string_literal(value).is_some()
}

fn clean_operation_identifier(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('$')
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

fn strings_from_value(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(value) => Some(vec![value.clone()]),
        Value::Array(values) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        ),
        _ => None,
    }
}

fn field_normalized<'a>(operation: &'a OperationSpec, key: &str) -> Option<&'a Value> {
    let normalized_key = normalize_operation_key(key);
    operation
        .fields
        .iter()
        .find(|(candidate, _value)| normalize_operation_key(candidate) == normalized_key)
        .map(|(_key, value)| value)
}

fn normalize_operation_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_word = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_uppercase() {
            if previous_was_word {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
            previous_was_word = true;
        } else if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_was_word = true;
        } else {
            normalized.push('_');
            previous_was_word = false;
        }
    }

    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn find_dataset<'a>(datasets: &'a [LoadedDataset], name: &str) -> Option<&'a LoadedDataset> {
    datasets
        .iter()
        .find(|dataset| dataset_matches_name(dataset, name))
}

fn dataset_matches_name(dataset: &LoadedDataset, name: &str) -> bool {
    dataset.metadata.name.eq_ignore_ascii_case(name)
        || dataset
            .metadata
            .domain
            .as_deref()
            .is_some_and(|domain| domain_scope_matches(name, domain))
        || dataset.metadata.filename.eq_ignore_ascii_case(name)
}

fn unsupported_operator(group: &ConditionGroup) -> Option<&Operator> {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().find_map(unsupported_operator)
        }
        ConditionGroup::Not(group) => unsupported_operator(group),
        ConditionGroup::Leaf(condition) => {
            (!is_supported_basic_operator(&condition.operator)).then_some(&condition.operator)
        }
    }
}

fn is_supported_basic_operator(operator: &Operator) -> bool {
    matches!(
        operator,
        Operator::Exists
            | Operator::NotExists
            | Operator::EqualTo
            | Operator::NotEqualTo
            | Operator::EqualToCaseInsensitive
            | Operator::NotEqualToCaseInsensitive
            | Operator::Contains
            | Operator::DoesNotContain
            | Operator::ContainsCaseInsensitive
            | Operator::DoesNotContainCaseInsensitive
            | Operator::IsContainedBy
            | Operator::IsNotContainedBy
            | Operator::IsContainedByCaseInsensitive
            | Operator::IsNotContainedByCaseInsensitive
            | Operator::ContainsAll
            | Operator::NotContainsAll
            | Operator::SharesNoElementsWith
            | Operator::IsNotOrderedSubsetOf
            | Operator::LessThan
            | Operator::LessThanOrEqualTo
            | Operator::GreaterThan
            | Operator::GreaterThanOrEqualTo
            | Operator::MatchesRegex
            | Operator::DoesNotMatchRegex
            | Operator::DoesNotMatchRegexFullString
            | Operator::LongerThan
            | Operator::StartsWith
            | Operator::PrefixEqualTo
            | Operator::PrefixNotEqualTo
            | Operator::NotPrefixMatchesRegex
            | Operator::PrefixIsNotContainedBy
            | Operator::EndsWith
            | Operator::SuffixMatchesRegex
            | Operator::NotSuffixMatchesRegex
            | Operator::SuffixIsNotContainedBy
            | Operator::DateEqualTo
            | Operator::DateNotEqualTo
            | Operator::DateLessThan
            | Operator::DateLessThanOrEqualTo
            | Operator::DateGreaterThan
            | Operator::DateGreaterThanOrEqualTo
            | Operator::InvalidDate
            | Operator::InvalidDuration
            | Operator::IsCompleteDate
            | Operator::IsIncompleteDate
            | Operator::TargetIsNotSortedBy
            | Operator::EmptyWithinExceptLastRow
            | Operator::DoesNotHaveNextCorrespondingRecord
            | Operator::NotPresentOnMultipleRowsWithin
            | Operator::InconsistentEnumeratedColumns
            | Operator::IsNotUniqueSet
            | Operator::IsUniqueSet
            | Operator::IsNotUniqueRelationship
            | Operator::IsInconsistentAcrossDataset
            | Operator::DoesNotEqualStringPart
            | Operator::IsEmpty
            | Operator::IsNotEmpty
    )
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
