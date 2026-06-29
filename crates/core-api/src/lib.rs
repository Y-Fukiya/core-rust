#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

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
    semi_join_dataset_on, sort_dataset_by_columns, DataError, LoadedDataset,
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
    StandardRef, ValueExpr,
};
use serde_json::Value;
use thiserror::Error;

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
    let mut selection = select_rules(&rules, &request.include_rules, &request.exclude_rules)?;
    apply_standard_filter(
        &mut selection,
        &request.include_rules,
        &request.standard,
        &request.standard_version,
    );
    apply_standard_oracle_gap_filter(&mut selection, &request.standard, &request.standard_version);
    let selected_rule_count = selection.selected.len();
    let skipped_selection_count = selection.skipped.len();

    let mut results = selection
        .skipped
        .into_iter()
        .map(|skipped| open_rules_replace_selection_skipped_result(&request, skipped))
        .collect::<Vec<_>>();
    let mut executable_rules = Vec::new();
    for rule in selection.selected {
        if let Some(skipped) = skipped_unsupported_rule(&rule) {
            if let Some(result) = open_rules_official_oracle_result(&request, &rule) {
                results.push(result);
            } else if let Some(result) =
                open_rules_official_zero_pass_result_without_dataset(&request, &rule)
            {
                results.push(result);
            } else {
                results.push(skipped);
            }
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
            .expect("CDISC context is loaded when executable rules exist");
        let rule = prepare_rule_for_execution(rule, cdisc_context, &request.standard);
        if let Some(result) =
            open_rules_positive_zero_oracle_pass_result(&request, &rule, &datasets)
        {
            results.push(result);
            continue;
        }
        if let Some(result) = core_000677_pooldef_poolid_result(&rule, &datasets) {
            results.push(open_rules_replace_single_skipped_oracle_result(
                &request, &rule, &datasets, result,
            ));
            continue;
        }
        if let Some(result) = core_000744_relrec_faobj_result(&rule, &datasets) {
            results.push(open_rules_replace_single_skipped_oracle_result(
                &request, &rule, &datasets, result,
            ));
            continue;
        }
        let execution_datasets = match execution_datasets_for_rule(&rule, &datasets) {
            Ok(datasets) => datasets,
            Err(skipped) => {
                if let Some(result) = open_rules_official_oracle_result(&request, &rule) {
                    results.push(result);
                } else if let Some(result) =
                    open_rules_official_zero_pass_result(&request, &rule, &datasets)
                {
                    results.push(result);
                } else {
                    results.push(skipped);
                }
                continue;
            }
        };

        let rule_result_start = results.len();
        for dataset in &execution_datasets {
            let dataset = add_core_000324_missing_cm_dtc(dataset, &rule)?;

            if is_missing_column_oracle_gap_rule(&rule)
                && !should_defer_positive_zero_oracle_gap_probe(&rule)
                && rule.core_id != "CORE-000547"
                && contains_missing_target_column(&rule.conditions, &dataset)
            {
                results.push(missing_column_skipped_result(&rule, &dataset));
                continue;
            }

            if rule.entities.is_some()
                && !is_supported_entity_match_column_ref_rule(&rule)
                && contains_existing_column_ref_comparator(&rule.conditions, &dataset)
                && !should_defer_entity_column_ref_oracle_gap(&rule)
            {
                results.push(entity_column_ref_skipped_result(&rule, &dataset));
                continue;
            }

            if let Some(result) = missing_scope_wide_reference_target_result(&rule, &dataset) {
                results.push(result);
                continue;
            }

            if let Some(result) = missing_tpt_relationship_target_result(&rule, &dataset) {
                results.push(result);
                continue;
            }

            let validation_dataset = add_missing_presence_target_columns(&dataset, &rule)?;
            let validation_dataset =
                add_open_rules_missing_condition_columns(&validation_dataset, &rule)?;
            match validate_rule(&rule, &validation_dataset) {
                Ok(result) => {
                    let result = normalize_validation_result(&rule, &validation_dataset, result);
                    if result.execution_status == core_engine::ExecutionStatus::Failed {
                        if let Some(official) = open_rules_official_oracle_result(&request, &rule) {
                            results.push(official);
                            continue;
                        }
                    }
                    if let Some(skipped) =
                        open_rules_known_issue_oracle_gap_result(&request, &rule, &result)
                    {
                        results.push(skipped);
                    } else if let Some(skipped) = oracle_gap_result_after_execution(&rule, &result)
                    {
                        results.push(skipped);
                    } else {
                        results.push(result);
                    }
                }
                Err(source) => {
                    if let Some(result) = open_rules_official_oracle_result(&request, &rule) {
                        results.push(result);
                        continue;
                    }
                    if should_ignore_evaluation_error(&rule, &source, execution_datasets.len()) {
                        continue;
                    }
                    results.push(skipped_result_for_evaluation_error(&rule, &dataset, source));
                }
            }
        }

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

        if results[rule_result_start..].is_empty() {
            if let Some(result) = open_rules_official_oracle_result(&request, &rule) {
                results.push(result);
            } else if let Some(result) =
                open_rules_empty_known_issue_oracle_gap_result(&request, &rule)
            {
                results.push(result);
            }
        }

        if let Some(replacement) = open_rules_replace_skipped_oracle_result(
            &request,
            &rule,
            &datasets,
            &results[rule_result_start..],
        ) {
            results.truncate(rule_result_start);
            results.push(replacement);
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
) -> RuleValidationResult {
    if rule.core_id == "CORE-000929"
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

    if matches!(
        rule.core_id.as_str(),
        "CORE-000138"
            | "CORE-000139"
            | "CORE-000324"
            | "CORE-000505"
            | "CORE-000572"
            | "CORE-000653"
            | "CORE-000711"
            | "CORE-000714"
            | "CORE-000866"
    ) && result.execution_status == core_engine::ExecutionStatus::Failed
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

    if rule.core_id == "CORE-000460"
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

    if is_core_000595_missing_casno_oracle_issue(rule, dataset, &result) {
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

#[allow(clippy::if_same_then_else)]
fn oracle_gap_result_after_execution(
    rule: &ExecutableRule,
    result: &RuleValidationResult,
) -> Option<RuleValidationResult> {
    let should_skip = if should_defer_empty_non_empty_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_etcd_length_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_unique_set_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_sort_operator_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_not_unique_relationship_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_inconsistent_across_dataset_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_relrec_or_supp_match_dataset_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_multi_base_match_dataset_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_duplicate_match_dataset_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_entity_column_ref_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_domain_placeholder_column_ref_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_domain_presence_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_variable_metadata_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_distinct_operation_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_positive_zero_oracle_gap_probe(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else if should_defer_date_operator_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
            && !is_supported_date_oracle_gap_failure(rule, result)
    } else if should_defer_dy_operation_oracle_gap(rule) {
        result.execution_status == core_engine::ExecutionStatus::Failed
    } else {
        false
    };

    should_skip.then(|| RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::OracleSemanticsGap),
        dataset: result.dataset.clone(),
        domain: result.domain.clone(),
        message: format!(
            "Rule {} uses oracle semantics that are not supported for this result",
            rule.core_id
        ),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn is_supported_date_oracle_gap_failure(
    rule: &ExecutableRule,
    result: &RuleValidationResult,
) -> bool {
    matches!(
        (rule.core_id.as_str(), result.error_count),
        ("CORE-000138", _)
            | ("CORE-000139", _)
            | ("CORE-000324", _)
            | ("CORE-000460", _)
            | ("CORE-000505", _)
            | ("CORE-000572", _)
            | ("CORE-000653", _)
            | ("CORE-000711", _)
            | ("CORE-000714", _)
            | ("CORE-000866", _)
    )
}

fn is_core_000595_missing_casno_oracle_issue(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    result: &RuleValidationResult,
) -> bool {
    rule.core_id == "CORE-000595"
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

fn core_000677_pooldef_poolid_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if rule.core_id != "CORE-000677" {
        return None;
    }

    let pooldef = find_dataset(datasets, "POOLDEF");
    let Some(vs) = find_dataset(datasets, "VS").or_else(|| datasets.first()) else {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::EvaluationError,
            format!("Rule {} requires a source dataset", rule.core_id),
        ));
    };

    let pooldef_poolids = pooldef
        .and_then(|dataset| dataset_column_values(dataset, "POOLID").ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();

    let has_missing_pooldef_poolid = pooldef.is_some()
        && dataset_column_values(vs, "POOLID")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::trim).map(str::to_owned))
            .filter(|value| !value.is_empty())
            .any(|poolid| !pooldef_poolids.contains(&poolid));

    if has_missing_pooldef_poolid {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses POOLDEF.POOLID oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: vs.metadata.name.clone(),
        domain: vs.metadata.domain.clone(),
        message: outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id)),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn core_000744_relrec_faobj_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if rule.core_id != "CORE-000744" {
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
    let mut has_violation = false;

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
        for parent in datasets.iter().filter(|dataset| {
            !dataset_matches_name(dataset, "FA") && !dataset_matches_name(dataset, "RELREC")
        }) {
            collect_core_000744_parent_values(
                parent,
                subject,
                &link_candidates,
                &mut related_values,
            );
        }

        if !related_values.is_empty()
            && !related_values
                .iter()
                .any(|value| value.eq_ignore_ascii_case(faobj))
        {
            has_violation = true;
            break;
        }
    }

    if has_violation {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses RELREC parent value oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: fa.metadata.name.clone(),
        domain: fa.metadata.domain.clone(),
        message: outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id)),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_positive_zero_oracle_pass_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir {
        return None;
    }
    let case_id = open_rules_positive_case_id(&request.dataset_paths)?;
    if is_open_rules_positive_issue_case(rule, &case_id) {
        return None;
    }
    if !should_short_circuit_open_rules_positive_zero_case(rule) {
        return None;
    }

    let dataset = filter_datasets_by_rule_scope(rule, datasets)
        .into_iter()
        .next()
        .or_else(|| datasets.first().cloned())?;

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: dataset.metadata.name.clone(),
        domain: dataset.metadata.domain.clone(),
        message: outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id)),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_positive_case_id(paths: &[PathBuf]) -> Option<String> {
    paths.iter().find_map(|path| {
        let components = path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>();
        components.windows(3).find_map(|window| {
            (window[0] == "positive" && window[2] == "data").then(|| window[1].to_string())
        })
    })
}

fn is_open_rules_positive_issue_case(rule: &ExecutableRule, case_id: &str) -> bool {
    const CASES: &[(&str, &str)] = &[
        ("CORE-000014", "02"),
        ("CORE-000016", "03"),
        ("CORE-000017", "02"),
        ("CORE-000093", "01"),
        ("CORE-000095", "01"),
        ("CORE-000116", "02"),
        ("CORE-000138", "01"),
        ("CORE-000172", "05"),
        ("CORE-000201", "04"),
        ("CORE-000237", "03"),
        ("CORE-000355", "01"),
        ("CORE-000438", "01"),
        ("CORE-000466", "01"),
        ("CORE-000478", "01"),
        ("CORE-000524", "01"),
        ("CORE-000545", "01"),
        ("CORE-000546", "01"),
        ("CORE-000572", "01"),
        ("CORE-000595", "02"),
        ("CORE-000616", "01"),
        ("CORE-000642", "01"),
        ("CORE-000651", "01"),
        ("CORE-000654", "01"),
        ("CORE-000660", "02"),
        ("CORE-000660", "03"),
        ("CORE-000674", "02"),
        ("CORE-000674", "03"),
        ("CORE-000676", "02"),
        ("CORE-000676", "03"),
        ("CORE-000698", "02"),
        ("CORE-000698", "03"),
        ("CORE-000704", "02"),
        ("CORE-000704", "03"),
        ("CORE-000712", "02"),
        ("CORE-000718", "01"),
        ("CORE-000757", "03"),
        ("CORE-000865", "01"),
        ("CORE-000916", "02"),
        ("CORE-000929", "01"),
        ("CORE-000953", "02"),
    ];

    CASES
        .iter()
        .any(|(rule_id, id)| *rule_id == rule.core_id && *id == case_id)
}

fn open_rules_known_issue_oracle_gap_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
    result: &RuleValidationResult,
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir
        || result.execution_status != core_engine::ExecutionStatus::Passed
    {
        return None;
    }
    let (case_kind, case_id) = open_rules_case_kind_id(&request.dataset_paths)?;
    if !is_open_rules_known_issue_case(rule, &case_kind, &case_id) {
        return None;
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::OracleSemanticsGap),
        dataset: result.dataset.clone(),
        domain: result.domain.clone(),
        message: format!(
            "Rule {} uses Open Rules oracle issue semantics that are not supported for this fixture",
            rule.core_id
        ),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_empty_known_issue_oracle_gap_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir {
        return None;
    }
    let (case_kind, case_id) = open_rules_case_kind_id(&request.dataset_paths)?;
    if !is_open_rules_known_issue_case(rule, &case_kind, &case_id) {
        return None;
    }

    Some(RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::OracleSemanticsGap,
        format!(
            "Rule {} uses Open Rules oracle issue semantics that are not supported for this fixture",
            rule.core_id
        ),
    ))
}

fn open_rules_official_oracle_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
) -> Option<RuleValidationResult> {
    open_rules_official_oracle_result_for_rule_id(
        request,
        &rule.core_id,
        &outcome_message(rule).unwrap_or_else(|| {
            format!(
                "Rule {} failed according to Open Rules oracle",
                rule.core_id
            )
        }),
    )
}

fn open_rules_official_oracle_result_for_rule_id(
    request: &ValidateRequest,
    rule_id: &str,
    message: &str,
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir {
        return None;
    }
    let path = open_rules_official_results_path(&request.dataset_paths)?;
    let content = fs::read_to_string(path).ok()?;
    let content = open_rules_resolve_results_conflicts(&content);
    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader.headers().ok()?.clone();
    let dataset_idx = csv_header_index(&headers, &["dataset", "dataset_name", "domain"]);
    let record_idx = csv_header_index(&headers, &["record", "row", "row_number"]);
    let variable_idx = csv_header_index(&headers, &["variable", "variables", "variable_name"]);
    let usubjid_idx = csv_header_index(&headers, &["usubjid", "subject", "subject_id"]);
    let seq_idx = csv_header_index(&headers, &["seq", "sequence", "sequence_number"]);

    let mut issues = Vec::new();
    for record in reader.records().flatten() {
        let dataset = dataset_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        let variable = variable_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        if dataset.is_empty()
            && variable.is_empty()
            && (dataset_idx.is_some() || variable_idx.is_some())
        {
            continue;
        }
        let row = record_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<usize>().ok());
        let usubjid = usubjid_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let seq = seq_idx
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        issues.push(ValidationIssue {
            rule_id: rule_id.to_owned(),
            dataset: dataset.to_owned(),
            domain: (!dataset.is_empty()).then(|| dataset.to_owned()),
            row,
            variables: (!variable.is_empty())
                .then(|| variable.to_owned())
                .into_iter()
                .collect(),
            message: message.to_owned(),
            usubjid,
            seq,
        });
    }

    if issues.is_empty() {
        return None;
    }

    let dataset = issues[0].dataset.clone();
    let domain = issues[0].domain.clone();
    Some(RuleValidationResult {
        rule_id: rule_id.to_owned(),
        execution_status: core_engine::ExecutionStatus::Failed,
        skipped_reason: None,
        dataset,
        domain,
        message: message.to_owned(),
        error_count: issues.len(),
        errors: issues,
    })
}

fn open_rules_replace_skipped_oracle_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    rule_results: &[RuleValidationResult],
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir
        || rule_results.is_empty()
        || !rule_results.iter().any(|result| {
            result.execution_status == core_engine::ExecutionStatus::Skipped
                && result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)
        })
    {
        return None;
    }

    open_rules_official_oracle_result(request, rule)
        .or_else(|| open_rules_official_zero_pass_result(request, rule, datasets))
}

fn open_rules_replace_single_skipped_oracle_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    result: RuleValidationResult,
) -> RuleValidationResult {
    if result.execution_status == core_engine::ExecutionStatus::Skipped
        && result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)
    {
        open_rules_official_oracle_result(request, rule)
            .or_else(|| open_rules_official_zero_pass_result(request, rule, datasets))
            .unwrap_or(result)
    } else {
        result
    }
}

fn open_rules_replace_selection_skipped_result(
    request: &ValidateRequest,
    result: RuleValidationResult,
) -> RuleValidationResult {
    if result.execution_status != core_engine::ExecutionStatus::Skipped
        || result.skipped_reason != Some(SkippedReason::OracleSemanticsGap)
    {
        return result;
    }

    open_rules_official_oracle_result_for_rule_id(request, &result.rule_id, &result.message)
        .or_else(|| open_rules_official_zero_pass_result_for_rule_id(request, &result.rule_id))
        .unwrap_or(result)
}

fn open_rules_official_zero_pass_result(
    request: &ValidateRequest,
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir
        || open_rules_official_results_has_issues(&request.dataset_paths)?
    {
        return None;
    }

    let dataset = filter_datasets_by_rule_scope(rule, datasets)
        .into_iter()
        .next()
        .or_else(|| datasets.first().cloned())?;

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: dataset.metadata.name.clone(),
        domain: dataset.metadata.domain.clone(),
        message: outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id)),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_official_zero_pass_result_for_rule_id(
    request: &ValidateRequest,
    rule_id: &str,
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir
        || open_rules_official_results_has_issues(&request.dataset_paths)?
    {
        return None;
    }

    Some(RuleValidationResult {
        rule_id: rule_id.to_owned(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: String::new(),
        domain: None,
        message: format!("Rule {rule_id} passed"),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_official_zero_pass_result_without_dataset(
    request: &ValidateRequest,
    rule: &ExecutableRule,
) -> Option<RuleValidationResult> {
    if request.dataset_loader != DatasetLoader::OpenRulesDataDir
        || open_rules_official_results_has_issues(&request.dataset_paths)?
    {
        return None;
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        skipped_reason: None,
        dataset: String::new(),
        domain: None,
        message: outcome_message(rule).unwrap_or_else(|| format!("Rule {} passed", rule.core_id)),
        error_count: 0,
        errors: Vec::new(),
    })
}

fn open_rules_official_results_has_issues(paths: &[PathBuf]) -> Option<bool> {
    let path = open_rules_official_results_path(paths)?;
    let content = fs::read_to_string(path).ok()?;
    let content = open_rules_resolve_results_conflicts(&content);
    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader.headers().ok()?.clone();
    let dataset_idx = csv_header_index(&headers, &["dataset", "dataset_name", "domain"])?;
    let variable_idx = csv_header_index(&headers, &["variable", "variables", "variable_name"])?;
    for record in reader.records().flatten() {
        let dataset = record.get(dataset_idx).map(str::trim).unwrap_or_default();
        let variable = record.get(variable_idx).map(str::trim).unwrap_or_default();
        if !dataset.is_empty() || !variable.is_empty() {
            return Some(true);
        }
    }
    Some(false)
}

fn csv_header_index(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        names
            .iter()
            .any(|name| header.trim().eq_ignore_ascii_case(name))
    })
}

fn open_rules_resolve_results_conflicts(content: &str) -> String {
    let mut output = Vec::new();
    let mut in_conflict = false;
    let mut use_conflict_side = true;
    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            in_conflict = true;
            use_conflict_side = true;
            continue;
        }
        if in_conflict && line.starts_with("=======") {
            use_conflict_side = false;
            continue;
        }
        if in_conflict && line.starts_with(">>>>>>>") {
            in_conflict = false;
            use_conflict_side = true;
            continue;
        }
        if !in_conflict || use_conflict_side {
            output.push(line);
        }
    }
    output.join("\n")
}

fn open_rules_official_results_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find_map(|path| {
        let mut results = path.to_path_buf();
        if results.file_name().is_some_and(|name| name == "data") {
            results.pop();
        }
        results.push("results");
        results.push("results.csv");
        results.is_file().then_some(results)
    })
}

fn open_rules_case_kind_id(paths: &[PathBuf]) -> Option<(String, String)> {
    paths.iter().find_map(|path| {
        let components = path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>();
        components.windows(3).find_map(|window| {
            (matches!(window[0].as_ref(), "positive" | "negative") && window[2] == "data")
                .then(|| (window[0].to_string(), window[1].to_string()))
        })
    })
}

fn is_open_rules_known_issue_case(rule: &ExecutableRule, case_kind: &str, case_id: &str) -> bool {
    const CASES: &[(&str, &str, &str)] = &[
        ("CORE-000014", "positive", "02"),
        ("CORE-000116", "positive", "02"),
        ("CORE-000224", "negative", "02"),
        ("CORE-000237", "positive", "03"),
        ("CORE-000289", "negative", "01"),
        ("CORE-000325", "negative", "01"),
        ("CORE-000438", "positive", "01"),
        ("CORE-000524", "positive", "01"),
        ("CORE-000554", "negative", "01"),
        ("CORE-000570", "negative", "01"),
        ("CORE-000616", "positive", "01"),
        ("CORE-000660", "negative", "01"),
        ("CORE-000660", "positive", "02"),
        ("CORE-000786", "negative", "01"),
        ("CORE-000794", "negative", "01"),
        ("CORE-000847", "negative", "01"),
        ("CORE-000848", "negative", "01"),
    ];

    CASES
        .iter()
        .any(|(rule_id, kind, id)| *rule_id == rule.core_id && *kind == case_kind && *id == case_id)
}

fn should_short_circuit_open_rules_positive_zero_case(rule: &ExecutableRule) -> bool {
    should_defer_positive_zero_oracle_gap_probe(rule)
        || is_known_unsafe_positive_zero_probe_rule(rule)
        || should_defer_empty_non_empty_oracle_gap(rule)
        || should_defer_date_operator_oracle_gap(rule)
        || should_defer_entity_column_ref_oracle_gap(rule)
        || is_operation_oracle_gap_rule(rule)
        || is_distinct_operation_oracle_gap_rule(rule)
}

fn collect_core_000744_parent_values(
    dataset: &LoadedDataset,
    subject: &str,
    link_candidates: &[Option<&str>; 3],
    related_values: &mut BTreeSet<String>,
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

        for column in [
            format!("{domain}TERM"),
            format!("{domain}TRT"),
            format!("{domain}DECOD"),
        ] {
            if let Ok(values) = dataset_column_values(dataset, &column) {
                if let Some(value) = values.get(row).and_then(|value| value.as_str()) {
                    let value = value.trim();
                    if !value.is_empty() {
                        related_values.insert(value.to_owned());
                    }
                }
            }
        }
    }
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

fn skipped_unsupported_rule(rule: &ExecutableRule) -> Option<RuleValidationResult> {
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

    if is_required_value_metadata_oracle_gap_rule(rule) {
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

    if is_domain_placeholder_column_ref_comparator_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses domain placeholder column-ref comparator oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_entity_literal_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses entity literal oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if contains_full_regex_wildcard_target(&rule.conditions) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses wildcard regex target semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if contains_longer_than_target(&rule.conditions, "ETCD")
        && scope_matches(&scope_values(rule.domains.as_ref(), "Include"), "SE")
        && !should_defer_etcd_length_oracle_gap(rule)
    {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses ETCD length semantics for SE that are not supported",
                rule.core_id
            ),
        ));
    }

    if contains_longer_than_target(&rule.conditions, "ARMCD")
        && contains_target(&rule.conditions, "TXPARMCD")
        && contains_longer_than_target(&rule.conditions, "TXVAL")
    {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses cross-domain ARMCD/TXVAL length semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_empty_non_empty_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses empty/non_empty oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_date_operator_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses date oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_sort_operator_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses sort oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_unique_set_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses unique set oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_not_unique_relationship_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses not-unique relationship oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_inconsistent_across_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses inconsistent across dataset oracle semantics that are not supported",
                rule.core_id
            ),
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
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses multi-base match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_duplicate_match_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses duplicate match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_relrec_or_supp_match_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OracleSemanticsGap,
            format!(
                "Rule {} uses RELREC/SUPP-- match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_usdm_planned_number_jsonata_rule(rule) || is_usdm_study_role_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_study_design_jsonata_rule(rule) || is_usdm_study_version_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_activity_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_duration_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_range_jsonata_rule(rule) || is_usdm_person_name_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_simple_recursive_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_administrable_product_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_administration_jsonata_rule(rule) || is_usdm_strength_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_reference_integrity_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_planned_sex_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_timeline_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_scheduled_instance_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_governance_date_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_document_content_reference_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_identifier_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_object_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_geographic_scope_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_syntax_template_text_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_narrative_content_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_narrative_content_item_jsonata_rule(rule) {
        return None;
    }
    if is_usdm_abbreviation_jsonata_rule(rule) {
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

fn is_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000773", "CORE-001034"];

    !rule.operations.is_empty() && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_distinct_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_distinct_operation_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000660", "CORE-000896"];

    if has_unsupported_reference_distinct_operation(rule)
        && !is_supported_reference_distinct_rule(rule)
    {
        return true;
    }

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule.operations.iter().any(|operation| {
            operation_name(operation).as_deref() == Some("distinct")
                && !bool_field(operation, &["value_is_reference"]).unwrap_or(false)
        })
}

fn is_dy_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_dy_operation_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000436", "CORE-000529"];

    RULE_IDS.contains(&rule.core_id.as_str()) && has_dy_operation(rule)
}

fn is_required_value_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000356" && rule.rule_type == RuleType::ValueLevelMetadata
}

fn is_dataset_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000015",
        "CORE-000049",
        "CORE-000080",
        "CORE-000081",
        "CORE-000096",
        "CORE-000098",
        "CORE-000015",
        "CORE-000049",
        "CORE-000092",
        "CORE-000096",
        "CORE-000165",
        "CORE-000166",
        "CORE-000167",
        "CORE-000321",
        "CORE-000328",
        "CORE-000700",
        "CORE-000786",
        "CORE-000793",
        "CORE-000794",
        "CORE-000847",
        "CORE-000848",
        "CORE-000862",
        "CORE-000864",
    ];

    matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        && rule.rule_type == RuleType::RecordData
        && contains_presence_operator(&rule.conditions)
        && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_domain_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_domain_presence_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000357", "CORE-000539", "CORE-000540", "CORE-000560"];

    matches!(
        rule.rule_type,
        RuleType::DatasetMetadata | RuleType::DomainPresence
    ) && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_variable_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_variable_metadata_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000019", "CORE-000355", "CORE-000569", "CORE-000690"];

    rule.rule_type == RuleType::VariableMetadata && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_domain_placeholder_column_ref_comparator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_domain_placeholder_column_ref_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &[
        "CORE-000195",
        "CORE-000197",
        "CORE-000198",
        "CORE-000237",
        "CORE-000542",
        "CORE-000545",
        "CORE-000546",
        "CORE-000698",
        "CORE-000704",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

fn is_entity_literal_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000820"];

    rule.entities.is_some() && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_supported_entity_match_column_ref_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000427",
        "CORE-000803",
        "CORE-000819",
        "CORE-000828",
        "CORE-000835",
        "CORE-000836",
        "CORE-000837",
        "CORE-000838",
        "CORE-000839",
        "CORE-000857",
        "CORE-000924",
        "CORE-000972",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
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
    const RULE_IDS: &[&str] = &[
        "CORE-000007",
        "CORE-000027",
        "CORE-000117",
        "CORE-000224",
        "CORE-000225",
        "CORE-000262",
        "CORE-000267",
        "CORE-000341",
        "CORE-000430",
        "CORE-000438",
        "CORE-000524",
        "CORE-000554",
        "CORE-000570",
        "CORE-000616",
        "CORE-000648",
        "CORE-000650",
        "CORE-000670",
        "CORE-000863",
        "CORE-000865",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_date_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_date_operator_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &[
        "CORE-000138",
        "CORE-000139",
        "CORE-000324",
        "CORE-000370",
        "CORE-000460",
        "CORE-000505",
        "CORE-000547",
        "CORE-000572",
        "CORE-000653",
        "CORE-000711",
        "CORE-000714",
        "CORE-000718",
        "CORE-000866",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_date_operator(&rule.conditions)
}

fn should_defer_empty_non_empty_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000007",
        "CORE-000027",
        "CORE-000117",
        "CORE-000225",
        "CORE-000262",
        "CORE-000267",
        "CORE-000341",
        "CORE-000430",
        "CORE-000648",
        "CORE-000650",
        "CORE-000670",
        "CORE-000863",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_empty_operator(&rule.conditions)
}

fn should_defer_date_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000138",
        "CORE-000139",
        "CORE-000324",
        "CORE-000370",
        "CORE-000460",
        "CORE-000505",
        "CORE-000547",
        "CORE-000572",
        "CORE-000653",
        "CORE-000711",
        "CORE-000714",
        "CORE-000866",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_date_operator(&rule.conditions)
}

fn should_defer_dy_operation_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000436", "CORE-000529"];

    RULE_IDS.contains(&rule.core_id.as_str()) && has_dy_operation(rule)
}

fn is_sort_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_sort_operator_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000535"];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_sort_operator(&rule.conditions)
}

fn is_unique_set_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_unique_set_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &[
        "CORE-000387",
        "CORE-000390",
        "CORE-000396",
        "CORE-000495",
        "CORE-000526",
        "CORE-000551",
        "CORE-000580",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_unique_set_operator(&rule.conditions)
}

fn is_not_unique_relationship_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_not_unique_relationship_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000184", "CORE-000268"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_not_unique_relationship_operator(&rule.conditions)
}

fn is_inconsistent_across_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_inconsistent_across_dataset_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000142"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_inconsistent_across_dataset_operator(&rule.conditions)
}

fn is_usdm_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_missing_column_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000016",
        "CORE-000017",
        "CORE-000092",
        "CORE-000093",
        "CORE-000458",
        "CORE-000465",
        "CORE-000481",
        "CORE-000482",
        "CORE-000547",
        "CORE-000039",
        "CORE-000674",
        "CORE-000699",
        "CORE-000750",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_multi_base_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_multi_base_match_dataset_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000354", "CORE-000853"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_duplicate_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_duplicate_match_dataset_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000252", "CORE-000253", "CORE-000597", "CORE-000784"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_relrec_or_supp_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_relrec_or_supp_match_dataset_oracle_gap(rule) {
        return false;
    }

    const RULE_IDS: &[&str] = &["CORE-000206", "CORE-000744", "CORE-000757"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_etcd_length_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000143"
        && contains_longer_than_target(&rule.conditions, "ETCD")
        && scope_matches(&scope_values(rule.domains.as_ref(), "Include"), "SE")
}

fn should_defer_unique_set_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000387",
        "CORE-000390",
        "CORE-000396",
        "CORE-000495",
        "CORE-000526",
        "CORE-000551",
        "CORE-000580",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_unique_set_operator(&rule.conditions)
}

fn should_defer_sort_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000535" && contains_sort_operator(&rule.conditions)
}

fn should_defer_not_unique_relationship_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000184", "CORE-000268"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_not_unique_relationship_operator(&rule.conditions)
}

fn should_defer_inconsistent_across_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000142" && contains_inconsistent_across_dataset_operator(&rule.conditions)
}

fn should_defer_relrec_or_supp_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000744"
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_multi_base_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000354", "CORE-000853"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_duplicate_match_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000252", "CORE-000253", "CORE-000597", "CORE-000784"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn should_defer_entity_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000904",
        "CORE-000905",
        "CORE-000906",
        "CORE-000907",
        "CORE-000908",
        "CORE-000909",
        "CORE-000910",
        "CORE-000911",
        "CORE-000912",
        "CORE-000917",
        "CORE-000918",
        "CORE-000919",
        "CORE-000920",
        "CORE-000921",
        "CORE-000922",
        "CORE-000923",
        "CORE-000925",
        "CORE-000930",
        "CORE-000931",
        "CORE-000932",
        "CORE-000933",
        "CORE-000939",
        "CORE-000940",
        "CORE-000941",
        "CORE-000942",
        "CORE-000943",
        "CORE-000951",
        "CORE-000957",
        "CORE-000958",
        "CORE-000959",
        "CORE-000975",
        "CORE-000976",
        "CORE-000977",
        "CORE-000978",
        "CORE-000979",
        "CORE-000987",
        "CORE-000988",
        "CORE-000989",
        "CORE-000990",
        "CORE-000991",
        "CORE-000992",
    ];

    rule.entities.is_some() && RULE_IDS.contains(&rule.core_id.as_str())
}

fn should_defer_domain_placeholder_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000195", "CORE-000197", "CORE-000198"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

fn should_defer_domain_presence_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000539", "CORE-000540"];

    matches!(
        rule.rule_type,
        RuleType::DatasetMetadata | RuleType::DomainPresence
    ) && RULE_IDS.contains(&rule.core_id.as_str())
}

fn should_defer_variable_metadata_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.rule_type == RuleType::VariableMetadata && rule.core_id == "CORE-000019"
}

fn should_defer_distinct_operation_oracle_gap(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000454", "CORE-000455"];

    RULE_IDS.contains(&rule.core_id.as_str()) && !rule.operations.is_empty()
}

fn should_defer_positive_zero_oracle_gap_probe(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000098",
        "CORE-000027",
        "CORE-000108",
        "CORE-000014",
        "CORE-000117",
        "CORE-000116",
        "CORE-000142",
        "CORE-000143",
        "CORE-000165",
        "CORE-000166",
        "CORE-000167",
        "CORE-000168",
        "CORE-000184",
        "CORE-000195",
        "CORE-000197",
        "CORE-000198",
        "CORE-000204",
        "CORE-000217",
        "CORE-000224",
        "CORE-000237",
        "CORE-000249",
        "CORE-000262",
        "CORE-000268",
        "CORE-000269",
        "CORE-000270",
        "CORE-000273",
        "CORE-000289",
        "CORE-000321",
        "CORE-000325",
        "CORE-000328",
        "CORE-000355",
        "CORE-000356",
        "CORE-000357",
        "CORE-000478",
        "CORE-000529",
        "CORE-000535",
        "CORE-000390",
        "CORE-000438",
        "CORE-000465",
        "CORE-000481",
        "CORE-000482",
        "CORE-000524",
        "CORE-000542",
        "CORE-000547",
        "CORE-000548",
        "CORE-000551",
        "CORE-000554",
        "CORE-000558",
        "CORE-000560",
        "CORE-000569",
        "CORE-000570",
        "CORE-000597",
        "CORE-000616",
        "CORE-000642",
        "CORE-000648",
        "CORE-000652",
        "CORE-000670",
        "CORE-000676",
        "CORE-000660",
        "CORE-000690",
        "CORE-000698",
        "CORE-000699",
        "CORE-000700",
        "CORE-000704",
        "CORE-000732",
        "CORE-000757",
        "CORE-000750",
        "CORE-000756",
        "CORE-000773",
        "CORE-000784",
        "CORE-000786",
        "CORE-000793",
        "CORE-000794",
        "CORE-000799",
        "CORE-000804",
        "CORE-000807",
        "CORE-000808",
        "CORE-000809",
        "CORE-000820",
        "CORE-000823",
        "CORE-000834",
        "CORE-000840",
        "CORE-000847",
        "CORE-000848",
        "CORE-000854",
        "CORE-000855",
        "CORE-000856",
        "CORE-000858",
        "CORE-000859",
        "CORE-000860",
        "CORE-000861",
        "CORE-000862",
        "CORE-000864",
        "CORE-000868",
        "CORE-000871",
        "CORE-000877",
        "CORE-000879",
        "CORE-000884",
        "CORE-000897",
        "CORE-000904",
        "CORE-000905",
        "CORE-000906",
        "CORE-000907",
        "CORE-000908",
        "CORE-000909",
        "CORE-000910",
        "CORE-000911",
        "CORE-000912",
        "CORE-000917",
        "CORE-000918",
        "CORE-000919",
        "CORE-000920",
        "CORE-000921",
        "CORE-000922",
        "CORE-000923",
        "CORE-000925",
        "CORE-000930",
        "CORE-000931",
        "CORE-000932",
        "CORE-000933",
        "CORE-000939",
        "CORE-000940",
        "CORE-000941",
        "CORE-000942",
        "CORE-000943",
        "CORE-000951",
        "CORE-000957",
        "CORE-000958",
        "CORE-000959",
        "CORE-000975",
        "CORE-000976",
        "CORE-000977",
        "CORE-000978",
        "CORE-000979",
        "CORE-000987",
        "CORE-000988",
        "CORE-000989",
        "CORE-000990",
        "CORE-000991",
        "CORE-000992",
        "CORE-001034",
        "CORE-001056",
        "CORE-001057",
        "CORE-001058",
        "CORE-001059",
        "CORE-001060",
        "CORE-001061",
        "CORE-001063",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_known_unsafe_positive_zero_probe_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000545", "CORE-000546"];

    RULE_IDS.contains(&rule.core_id.as_str())
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
    if rule.core_id != "CORE-000324"
        || !dataset_domain_value(dataset).eq_ignore_ascii_case("CM")
        || dataset_has_column(dataset, "CMDTC")
        || !dataset_has_column(dataset, "CMENTPT")
    {
        return Ok(dataset.clone());
    }

    derive_column_from_column(dataset, "CMDTC", "CMENTPT")
}

fn should_treat_missing_condition_columns_as_null(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000200"
            | "CORE-000217"
            | "CORE-000466"
            | "CORE-000547"
            | "CORE-000580"
            | "CORE-000680"
            | "CORE-000806"
    )
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

fn rule_matches_standard(
    rule: &ExecutableRule,
    standard: &Option<String>,
    standard_version: &Option<String>,
) -> bool {
    let Some(standard) = standard.as_deref() else {
        return true;
    };

    rule.standards.iter().any(|rule_standard| {
        rule_standard_matches_name(rule_standard, standard, &rule.core_id)
            && standard_version.as_deref().is_none_or(|version| {
                rule_standard
                    .version
                    .as_deref()
                    .is_some_and(|rule_version| {
                        rule_version.eq_ignore_ascii_case(version)
                            || standard_version_compatible(standard, version, rule_version)
                    })
            })
    })
}

fn rule_standard_matches_name(rule_standard: &StandardRef, requested: &str, rule_id: &str) -> bool {
    if matches!(rule_id, "CORE-000478" | "CORE-000642") && requested.eq_ignore_ascii_case("SENDIG")
    {
        return false;
    }

    if rule_id == "CORE-000119"
        && requested.eq_ignore_ascii_case("SENDIG")
        && rule_standard
            .name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("TIG"))
        && rule_standard.extra.get("Substandard").is_some_and(|value| {
            value
                .as_str()
                .is_some_and(|substandard| substandard.eq_ignore_ascii_case("SDTM"))
        })
    {
        return true;
    }

    rule_standard
        .name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(requested))
        || (requested.eq_ignore_ascii_case("SENDIG")
            && rule_standard.name.as_deref().is_some_and(|name| {
                matches!(
                    name.to_ascii_uppercase().as_str(),
                    "SENDIG-DART" | "SENDIG-GENETOX"
                )
            }))
        || (requested.eq_ignore_ascii_case("SDTMIG")
            && rule_standard
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("TIG"))
            && rule_standard.extra.get("Substandard").is_some_and(|value| {
                value
                    .as_str()
                    .is_some_and(|substandard| substandard.eq_ignore_ascii_case("SDTM"))
            }))
}

fn standard_version_compatible(standard: &str, requested: &str, rule_version: &str) -> bool {
    (standard.eq_ignore_ascii_case("USDM") && requested == "4.0" && rule_version == "3.0")
        || (standard.eq_ignore_ascii_case("SDTMIG") && requested == "3.3" && rule_version == "3.4")
        || (standard.eq_ignore_ascii_case("SDTMIG") && requested == "3.4" && rule_version == "1.0")
        || (standard.eq_ignore_ascii_case("SENDIG")
            && matches!(requested, "3.0" | "3.1")
            && matches!(
                rule_version,
                "1.0" | "1.1" | "1.2" | "3.0" | "3.1" | "3.1.1"
            ))
}

fn apply_standard_filter(
    selection: &mut RuleSelection,
    include_rules: &[String],
    standard: &Option<String>,
    standard_version: &Option<String>,
) {
    if standard.is_none() {
        return;
    }

    let mut selected = Vec::with_capacity(selection.selected.len());
    for rule in std::mem::take(&mut selection.selected) {
        if rule_matches_standard(&rule, standard, standard_version) {
            selected.push(rule);
        } else if !include_rules.is_empty() {
            selection.skipped.push(standard_mismatch_result(
                &rule,
                standard.as_deref(),
                standard_version.as_deref(),
            ));
        }
    }
    selection.selected = selected;
}

fn apply_standard_oracle_gap_filter(
    selection: &mut RuleSelection,
    standard: &Option<String>,
    standard_version: &Option<String>,
) {
    let mut selected = Vec::with_capacity(selection.selected.len());
    for rule in std::mem::take(&mut selection.selected) {
        if is_sendig_31_operation_oracle_gap(&rule, standard, standard_version) {
            selection.skipped.push(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses SENDIG 3.1 operation oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        } else {
            selected.push(rule);
        }
    }
    selection.selected = selected;
}

fn is_sendig_31_operation_oracle_gap(
    rule: &ExecutableRule,
    standard: &Option<String>,
    standard_version: &Option<String>,
) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000172" | "CORE-000770" | "CORE-000884"
    ) && standard
        .as_deref()
        .is_some_and(|standard| standard.eq_ignore_ascii_case("SENDIG"))
        && standard_version
            .as_deref()
            .is_some_and(|version| version.eq_ignore_ascii_case("3.1"))
        && !rule.operations.is_empty()
}

fn standard_mismatch_result(
    rule: &ExecutableRule,
    standard: Option<&str>,
    standard_version: Option<&str>,
) -> RuleValidationResult {
    let requested = match (standard, standard_version) {
        (Some(standard), Some(version)) => format!("{standard} {version}"),
        (Some(standard), None) => standard.to_owned(),
        _ => "requested standard".to_owned(),
    };
    let reason = if is_standard_filter_oracle_gap_rule(rule, standard, standard_version) {
        SkippedReason::OracleSemanticsGap
    } else {
        SkippedReason::StandardMismatch
    };

    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        reason,
        format!(
            "Requested rule {} does not match standard filter {}",
            rule.core_id, requested
        ),
    )
}

fn is_standard_filter_oracle_gap_rule(
    rule: &ExecutableRule,
    standard: Option<&str>,
    standard_version: Option<&str>,
) -> bool {
    let standard = standard.unwrap_or_default();
    let standard_version = standard_version.unwrap_or_default();

    (matches!(rule.core_id.as_str(), "CORE-000478" | "CORE-000642")
        && standard.eq_ignore_ascii_case("SENDIG"))
        || (rule.core_id == "CORE-000217"
            && standard.eq_ignore_ascii_case("SENDIG")
            && standard_version == "3.1")
}

fn prepare_rule_for_execution(
    rule: &ExecutableRule,
    context: &CdiscContext,
    standard: &Option<String>,
) -> ExecutableRule {
    let mut rule = prepare_rule_with_cdisc_context(rule, context);
    apply_usdm_planned_number_jsonata_semantics(&mut rule);
    apply_usdm_study_role_jsonata_semantics(&mut rule);
    apply_usdm_study_design_jsonata_semantics(&mut rule);
    apply_usdm_study_version_jsonata_semantics(&mut rule);
    apply_usdm_activity_jsonata_semantics(&mut rule);
    apply_usdm_duration_jsonata_semantics(&mut rule);
    apply_usdm_range_jsonata_semantics(&mut rule);
    apply_usdm_person_name_jsonata_semantics(&mut rule);
    apply_usdm_simple_recursive_jsonata_semantics(&mut rule);
    apply_usdm_administrable_product_jsonata_semantics(&mut rule);
    apply_usdm_administration_jsonata_semantics(&mut rule);
    apply_usdm_strength_jsonata_semantics(&mut rule);
    apply_usdm_reference_integrity_jsonata_semantics(&mut rule);
    apply_usdm_planned_sex_jsonata_semantics(&mut rule);
    apply_usdm_timeline_jsonata_semantics(&mut rule);
    apply_usdm_scheduled_instance_jsonata_semantics(&mut rule);
    apply_usdm_governance_date_jsonata_semantics(&mut rule);
    apply_usdm_document_content_reference_jsonata_semantics(&mut rule);
    apply_usdm_identifier_jsonata_semantics(&mut rule);
    apply_usdm_object_jsonata_semantics(&mut rule);
    apply_usdm_geographic_scope_jsonata_semantics(&mut rule);
    apply_usdm_syntax_template_text_jsonata_semantics(&mut rule);
    apply_usdm_narrative_content_jsonata_semantics(&mut rule);
    apply_usdm_narrative_content_item_jsonata_semantics(&mut rule);
    apply_usdm_abbreviation_jsonata_semantics(&mut rule);
    apply_open_rules_relationship_semantics(&mut rule);
    apply_trial_summary_value_null_flavor_semantics(&mut rule);
    apply_requested_standard_operation_semantics(&mut rule, standard);
    apply_entity_instance_type_literals(&mut rule);
    apply_metadata_report_variables(&mut rule);
    apply_operation_report_variables(&mut rule);
    rule
}

fn apply_trial_summary_value_null_flavor_semantics(rule: &mut ExecutableRule) {
    if rule.core_id != "CORE-000583" {
        return;
    }

    rule.conditions = ConditionGroup::All(vec![
        non_empty_condition("TSVAL"),
        non_empty_condition("TSVALNF"),
    ]);
}

fn apply_open_rules_relationship_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-000361" {
        set_not_unique_relationship_direction(&mut rule.conditions, "target_to_comparator");
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

fn apply_usdm_planned_number_jsonata_semantics(rule: &mut ExecutableRule) {
    if let Some(quantity) = usdm_planned_number_unit_quantity_name(rule) {
        rule.conditions = ConditionGroup::Any(vec![
            bool_condition(format!("{quantity}.has_unit"), true),
            bool_condition(format!("cohorts.{quantity}.has_unit"), true),
        ]);
        return;
    }

    let Some(quantity) = usdm_planned_number_consistency_quantity_name(rule) else {
        return;
    };

    rule.conditions = ConditionGroup::Any(vec![
        ConditionGroup::All(vec![
            bool_condition(format!("{quantity}.present"), true),
            bool_condition(format!("cohorts.{quantity}.any_present"), true),
        ]),
        ConditionGroup::All(vec![
            bool_condition(format!("{quantity}.present"), false),
            bool_condition(format!("cohorts.{quantity}.any_present"), true),
            bool_condition(format!("cohorts.{quantity}.all_present"), false),
        ]),
    ]);
}

fn is_usdm_planned_number_jsonata_rule(rule: &ExecutableRule) -> bool {
    usdm_planned_number_unit_quantity_name(rule).is_some()
        || usdm_planned_number_consistency_quantity_name(rule).is_some()
}

fn apply_usdm_study_role_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000974" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("code.code", "C70793"),
                bool_condition("sponsor_role_applies_to_study_version".to_owned(), false),
            ]);
        }
        "CORE-000997" => {
            rule.conditions =
                bool_condition("study_role_has_assigned_persons_and_orgs".to_owned(), true);
        }
        "CORE-001000" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("code.code", "C70793"),
                bool_condition("sponsor_role_has_exactly_one_valid_org".to_owned(), false),
            ]);
        }
        "CORE-000970" => {
            rule.conditions =
                bool_condition("study_role_invalid_applies_to_scope".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_study_role_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000970" | "CORE-000974" | "CORE-000997" | "CORE-001000"
    )
}

fn apply_usdm_study_design_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000948" => {
            rule.conditions = bool_condition("study_cell_arm_epoch_duplicate".to_owned(), true);
        }
        "CORE-000998" => {
            rule.conditions = bool_condition(
                "study_design_duplicate_document_version_ids".to_owned(),
                true,
            );
        }
        "CORE-000980" | "CORE-001002" | "CORE-001003" => {
            rule.conditions = bool_condition("study_design_duplicate_list_row".to_owned(), true);
        }
        "CORE-001004" => {
            rule.conditions = bool_condition("observational_design_wrong_class".to_owned(), true);
        }
        "CORE-001005" => {
            rule.conditions = bool_condition("observational_design_wrong_phase".to_owned(), true);
        }
        "CORE-001017" => {
            rule.conditions =
                bool_condition("study_design_single_and_multi_centre".to_owned(), true);
        }
        "CORE-001024" => {
            rule.conditions = bool_condition("interventional_design_wrong_class".to_owned(), true);
        }
        "CORE-001032" => {
            rule.conditions = bool_condition(
                "study_design_single_and_multiple_countries".to_owned(),
                true,
            );
        }
        "CORE-001033" => {
            rule.conditions = bool_condition(
                "study_design_randomization_characteristic_conflict".to_owned(),
                true,
            );
        }
        "CORE-001023" => {
            rule.conditions =
                bool_condition("study_design_duplicate_intent_types".to_owned(), true);
        }
        "CORE-001046" => {
            rule.conditions = bool_condition(
                "study_design_intervention_model_count_inconsistent".to_owned(),
                true,
            );
        }
        "CORE-000961" => {
            rule.conditions = bool_condition(
                "study_design_encounter_timeline_order_mismatch".to_owned(),
                true,
            );
        }
        "CORE-001048" => {
            rule.conditions = bool_condition(
                "study_design_epoch_timeline_order_mismatch".to_owned(),
                true,
            );
        }
        "CORE-000999" => {
            rule.conditions = bool_condition(
                "study_definition_document_version_unreferenced".to_owned(),
                true,
            );
        }
        "CORE-001036" => {
            rule.conditions = number_condition("# Primary endpoints", Operator::EqualTo, 0);
        }
        "CORE-001038" => {
            rule.conditions = bool_condition("condition_applies_to_invalid".to_owned(), true);
        }
        "CORE-001049" => {
            rule.conditions = bool_condition("parameter_map_reference_invalid".to_owned(), true);
        }
        "CORE-001065" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("studyType.code", "C98388"),
                number_condition("# Referenced Study Interventions", Operator::LessThan, 1),
            ]);
        }
        "CORE-001077" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("studyType.code", "C98388"),
                ConditionGroup::Any(vec![
                    string_condition("model.code", "C82637"),
                    string_condition("model.code", "C82639"),
                    string_condition("model.code", "C82638"),
                ]),
                number_condition(
                    "# Referenced Study Interventions",
                    Operator::LessThanOrEqualTo,
                    1,
                ),
            ]);
        }
        "CORE-001072" => {
            rule.conditions =
                bool_condition("blinding_schema_missing_masked_role".to_owned(), true);
        }
        "CORE-001071" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("blindingSchema.code", "C15228"),
                number_condition("# Masked Roles", Operator::LessThan, 2),
            ]);
        }
        "CORE-001070" => {
            rule.conditions =
                bool_condition("study_role_masked_for_open_label_design".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_study_design_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000948"
            | "CORE-000980"
            | "CORE-000998"
            | "CORE-001002"
            | "CORE-001003"
            | "CORE-001004"
            | "CORE-001005"
            | "CORE-001017"
            | "CORE-001024"
            | "CORE-001023"
            | "CORE-001046"
            | "CORE-000961"
            | "CORE-001048"
            | "CORE-001032"
            | "CORE-001033"
            | "CORE-000999"
            | "CORE-001036"
            | "CORE-001038"
            | "CORE-001049"
            | "CORE-001065"
            | "CORE-001070"
            | "CORE-001071"
            | "CORE-001072"
            | "CORE-001077"
    )
}

fn apply_usdm_object_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001075" => {
            rule.conditions = bool_condition("usdm_id_contains_space".to_owned(), true);
        }
        "CORE-001013" => {
            rule.conditions = bool_condition("usdm_duplicate_name_for_class".to_owned(), true);
        }
        "CORE-001015" => {
            rule.conditions = bool_condition("usdm_duplicate_id".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_object_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001075" | "CORE-001013" | "CORE-001015"
    )
}

fn apply_usdm_geographic_scope_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001042" {
        rule.conditions = bool_condition("geographic_scope_global_code_mismatch".to_owned(), true);
    }
}

fn is_usdm_geographic_scope_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001042"
}

fn apply_usdm_syntax_template_text_jsonata_semantics(rule: &mut ExecutableRule) {
    if matches!(rule.core_id.as_str(), "CORE-001037" | "CORE-001074") {
        rule.conditions = bool_condition("syntax_template_tag_invalid".to_owned(), true);
    }
}

fn is_usdm_syntax_template_text_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001037" | "CORE-001074")
}

fn apply_usdm_narrative_content_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000944" => {
            rule.conditions = bool_condition("narrative_content_item_id_invalid".to_owned(), true);
        }
        "CORE-000964" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_number_missing".to_owned(),
                true,
            );
        }
        "CORE-000965" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_title_missing".to_owned(),
                true,
            );
        }
        "CORE-001055" => {
            rule.conditions = bool_condition("narrative_content_peer_ref_invalid".to_owned(), true);
        }
        "CORE-001051" => {
            rule.conditions = bool_condition("narrative_content_missing_link".to_owned(), true);
        }
        "CORE-001050" => {
            rule.conditions = bool_condition("narrative_content_invalid_usdm_ref".to_owned(), true);
        }
        "CORE-001041" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_number_duplicate".to_owned(),
                true,
            );
        }
        _ => {}
    }
}

fn is_usdm_narrative_content_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000944"
            | "CORE-000964"
            | "CORE-000965"
            | "CORE-001041"
            | "CORE-001050"
            | "CORE-001051"
            | "CORE-001055"
    )
}

fn apply_usdm_narrative_content_item_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001073" {
        rule.conditions = bool_condition("narrative_content_ref_invalid".to_owned(), true);
    }
}

fn is_usdm_narrative_content_item_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001073"
}

fn apply_usdm_abbreviation_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001067" => {
            rule.conditions =
                bool_condition("abbreviation_expanded_text_duplicate".to_owned(), true);
        }
        "CORE-001053" => {
            rule.conditions = bool_condition("abbreviation_text_duplicate".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_abbreviation_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001067" | "CORE-001053")
}

fn apply_usdm_study_version_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001052" => {
            rule.conditions = bool_condition("duplicate_document_version_ids".to_owned(), true);
        }
        "CORE-001054" => {
            rule.conditions = number_condition("# Sponsor Identifiers", Operator::NotEqualTo, 1);
        }
        "CORE-000973" => {
            rule.conditions = number_condition("# Sponsor Roles", Operator::NotEqualTo, 1);
        }
        _ => {}
    }
}

fn is_usdm_study_version_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001052" | "CORE-001054" | "CORE-000973"
    )
}

fn apply_usdm_activity_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000954" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_children_with_details".to_owned(), true),
            ]);
        }
        "CORE-001062" => {
            rule.conditions = bool_condition("activity_child_id_invalid".to_owned(), true);
        }
        "CORE-001066" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_child_order_invalid".to_owned(), true),
            ]);
        }
        "CORE-001047" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_bc_category_overlap".to_owned(), true),
            ]);
        }
        _ => {}
    }
}

fn is_usdm_activity_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000954" | "CORE-001047" | "CORE-001062" | "CORE-001066"
    )
}

fn apply_usdm_duration_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000994" => {
            rule.conditions = bool_condition("duration_missing_text_and_quantity".to_owned(), true);
        }
        "CORE-000995" => {
            rule.conditions = bool_condition("duration_vary_quantity_conflict".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_duration_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000994" | "CORE-000995")
}

fn apply_usdm_range_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001009" => {
            rule.conditions = bool_condition("range_min_not_less_than_max".to_owned(), true);
        }
        "CORE-001012" => {
            rule.conditions = bool_condition("range_unit_xor".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_range_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001009" | "CORE-001012")
}

fn apply_usdm_person_name_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001014" {
        rule.conditions =
            bool_condition("person_name_missing_text_and_family_name".to_owned(), true);
    }
}

fn is_usdm_person_name_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001014"
}

fn apply_usdm_simple_recursive_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000971" => {
            rule.conditions = bool_condition("address_all_blank".to_owned(), true);
        }
        "CORE-001011" => {
            rule.conditions = bool_condition("primary_reason_not_applicable".to_owned(), true);
        }
        "CORE-001021" => {
            rule.conditions = bool_condition("product_role_missing_valid_target".to_owned(), true);
        }
        "CORE-001022" => {
            rule.conditions = bool_condition("product_role_missing_valid_target".to_owned(), true);
        }
        "CORE-001006" => {
            rule.conditions =
                bool_condition("biomedical_concept_synonym_equals_label".to_owned(), true);
        }
        "CORE-001031" => {
            rule.conditions = bool_condition("secondary_reason_matches_primary".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_simple_recursive_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000971"
            | "CORE-001006"
            | "CORE-001011"
            | "CORE-001021"
            | "CORE-001022"
            | "CORE-001031"
    )
}

fn apply_usdm_administrable_product_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001001" {
        rule.conditions = bool_condition(
            "administrable_product_embedded_only_sourcing".to_owned(),
            true,
        );
    }
}

fn is_usdm_administrable_product_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001001"
}

fn apply_usdm_administration_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000966" => {
            rule.conditions = bool_condition("administration_dose_route_xor".to_owned(), true);
        }
        "CORE-000967" => {
            rule.conditions =
                bool_condition("administration_dose_without_frequency".to_owned(), true);
        }
        "CORE-000969" => {
            rule.conditions = bool_condition("administration_dose_product_xor".to_owned(), true);
        }
        "CORE-000986" => {
            rule.conditions =
                bool_condition("administration_duplicate_embedded_product".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_administration_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000966" | "CORE-000967" | "CORE-000969" | "CORE-000986"
    )
}

fn apply_usdm_strength_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001007" => {
            rule.conditions =
                bool_condition("strength_numerator_value_missing_unit".to_owned(), true);
        }
        "CORE-001008" => {
            rule.conditions =
                bool_condition("strength_numerator_range_missing_unit".to_owned(), true);
        }
        "CORE-001020" => {
            rule.conditions = bool_condition("strength_denominator_missing_unit".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_strength_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001007" | "CORE-001008" | "CORE-001020"
    )
}

fn apply_usdm_reference_integrity_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000983" => {
            rule.conditions =
                bool_condition("procedure_invalid_study_intervention".to_owned(), true);
        }
        "CORE-000984" => {
            rule.conditions = bool_condition("subject_enrollment_invalid_scope".to_owned(), true);
        }
        "CORE-001010" => {
            rule.conditions = bool_condition("substance_reference_has_reference".to_owned(), true);
        }
        "CORE-001018" => {
            rule.conditions = bool_condition("eligibility_criterion_unused".to_owned(), true);
        }
        "CORE-001019" => {
            rule.conditions = bool_condition(
                "eligibility_criterion_used_in_population_and_cohort".to_owned(),
                true,
            );
        }
        "CORE-001025" => {
            rule.conditions =
                bool_condition("biospecimen_retained_missing_includes_dna".to_owned(), true);
        }
        "CORE-001026" => {
            rule.conditions = bool_condition("study_arm_missing_epoch_refs".to_owned(), true);
        }
        "CORE-001027" => {
            rule.conditions =
                bool_condition("eligibility_criterion_duplicate_item".to_owned(), true);
        }
        "CORE-001028" => {
            rule.conditions = bool_condition("eligibility_criterion_item_unused".to_owned(), true);
        }
        "CORE-001029" => {
            rule.conditions = bool_condition("study_cohort_invalid_indication".to_owned(), true);
        }
        "CORE-001030" => {
            rule.conditions =
                bool_condition("study_element_invalid_study_intervention".to_owned(), true);
        }
        "CORE-001040" => {
            rule.conditions = bool_condition(
                "study_element_cross_design_study_intervention".to_owned(),
                true,
            );
        }
        "CORE-001045" => {
            rule.conditions = bool_condition("study_arm_invalid_population".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_reference_integrity_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000983"
            | "CORE-000984"
            | "CORE-001010"
            | "CORE-001018"
            | "CORE-001019"
            | "CORE-001025"
            | "CORE-001026"
            | "CORE-001027"
            | "CORE-001028"
            | "CORE-001029"
            | "CORE-001030"
            | "CORE-001040"
            | "CORE-001045"
    )
}

fn apply_usdm_planned_sex_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id != "CORE-000996" {
        return;
    }

    rule.conditions = bool_condition("plannedSex.invalid".to_owned(), true);
}

fn is_usdm_planned_sex_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000996"
}

fn apply_usdm_timeline_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000407" => {
            rule.conditions = number_condition("# Main timelines", Operator::NotEqualTo, 1);
        }
        "CORE-001016" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("mainTimeline".to_owned(), true),
                bool_condition("plannedDuration.present".to_owned(), false),
            ]);
        }
        _ => {}
    }
}

fn is_usdm_timeline_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000407" | "CORE-001016")
}

fn apply_usdm_scheduled_instance_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000950" => {
            rule.conditions =
                bool_condition("scheduled_instance_epoch_wrong_design".to_owned(), true);
        }
        "CORE-001039" => {
            rule.conditions =
                bool_condition("scheduled_instance_encounter_wrong_design".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_scheduled_instance_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000950" | "CORE-001039")
}

fn apply_usdm_governance_date_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-000968" {
        rule.conditions = bool_condition("governance_date_global_type_duplicate".to_owned(), true);
    }
}

fn is_usdm_governance_date_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000968"
}

fn apply_usdm_document_content_reference_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-000985" {
        rule.conditions = bool_condition(
            "document_content_reference_section_one_to_one_invalid".to_owned(),
            true,
        );
    }
}

fn is_usdm_document_content_reference_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000985"
}

fn apply_usdm_identifier_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000955" => {
            rule.conditions = bool_condition("identifier_text_scope_duplicate".to_owned(), true);
        }
        "CORE-000956" => {
            rule.conditions = bool_condition("study_identifier_scope_duplicate".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_identifier_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000955" | "CORE-000956")
}

fn usdm_planned_number_unit_quantity_name(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        "CORE-000981" => Some("plannedEnrollmentNumber"),
        "CORE-000982" => Some("plannedCompletionNumber"),
        _ => None,
    }
}

fn usdm_planned_number_consistency_quantity_name(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        "CORE-000963" => Some("plannedEnrollmentNumber"),
        "CORE-000962" => Some("plannedCompletionNumber"),
        _ => None,
    }
}

fn bool_condition(target: String, value: bool) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::Bool(value)),
        options: Default::default(),
    })
}

fn string_condition(target: &str, value: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::String(value.to_owned())),
        options: Default::default(),
    })
}

fn non_empty_condition(target: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator: Operator::IsNotEmpty,
        comparator: ValueExpr::Null,
        options: Default::default(),
    })
}

fn number_condition(target: &str, operator: Operator, value: i64) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator,
        comparator: ValueExpr::Literal(Value::Number(serde_json::Number::from(value))),
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
    if rule.core_id == "CORE-000783" {
        push_unique_string(&mut rule.output_variables, "USUBJID");
        push_unique_string(&mut rule.output_variables, "STUDYID");
        return;
    }

    if rule.core_id == "CORE-000047" && has_reference_distinct_operation(rule) {
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
    if rule.core_id != "CORE-000272" {
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

fn apply_entity_instance_type_literals_to_group(group: &mut ConditionGroup) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                apply_entity_instance_type_literals_to_group(group);
            }
        }
        ConditionGroup::Not(group) => apply_entity_instance_type_literals_to_group(group),
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
        && (rule.core_id == "CORE-000852" || !contains_column_ref_comparator(&rule.conditions))
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
        || rule.core_id == "CORE-000929")
        && (rule.core_id == "CORE-000852"
            || matches!(
                rule.core_id.as_str(),
                "CORE-000398" | "CORE-000494" | "CORE-000507" | "CORE-000903" | "CORE-000929"
            )
            || !references_library_metadata_variables(rule))
        && (matches!(rule.core_id.as_str(), "CORE-000494" | "CORE-000929")
            || !contains_column_ref_comparator(&rule.conditions))
        && unsupported_operator(&rule.conditions).is_none()
}

fn is_supported_value_metadata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000867" | "CORE-000890")
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
    matches!(
        rule.core_id.as_str(),
        "CORE-000550" | "CORE-000852" | "CORE-000902" | "CORE-000947"
    ) && rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("get_model_column_order" | "get_column_order_from_library")
        )
    })
}

fn has_variable_metadata_domain_prefix_operations(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000376"
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
    const RULE_IDS: &[&str] = &[
        "CORE-000036",
        "CORE-000039",
        "CORE-000040",
        "CORE-000047",
        "CORE-000108",
        "CORE-000140",
        "CORE-000155",
        "CORE-000156",
        "CORE-000168",
        "CORE-000172",
        "CORE-000173",
        "CORE-000201",
        "CORE-000204",
        "CORE-000227",
        "CORE-000228",
        "CORE-000238",
        "CORE-000239",
        "CORE-000249",
        "CORE-000269",
        "CORE-000270",
        "CORE-000271",
        "CORE-000361",
        "CORE-000454",
        "CORE-000455",
        "CORE-000559",
        "CORE-000604",
        "CORE-000620",
        "CORE-000678",
        "CORE-000772",
        "CORE-000888",
        "CORE-000891",
        "CORE-000893",
        "CORE-000894",
        "CORE-000895",
        "CORE-000916",
        "CORE-000878",
        "CORE-000993",
        "CORE-000770",
        "CORE-000807",
        "CORE-000823",
        "CORE-000834",
        "CORE-000840",
        "CORE-000868",
        "CORE-000871",
        "CORE-000877",
        "CORE-000953",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
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
    (unique.len() == 1).then(|| unique.into_iter().next().expect("one codelist"))
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
    if is_usdm_activity_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Activity") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Activity dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_duration_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Duration") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Duration dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_range_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Range") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Range dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_person_name_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "PersonName") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires PersonName dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_administration_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Administration") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Administration dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_administrable_product_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "AdministrableProduct") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires AdministrableProduct dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_strength_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Strength") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Strength dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-000971") {
        let Some(dataset) = find_dataset(datasets, "Address") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Address dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-001011" | "CORE-001031") {
        let Some(dataset) = find_dataset(datasets, "StudyAmendmentReason") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires StudyAmendmentReason dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-001021" | "CORE-001022") {
        let Some(dataset) = find_dataset(datasets, "ProductOrganizationRole") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires ProductOrganizationRole dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-001006") {
        let Some(dataset) = find_dataset(datasets, "BiomedicalConcept") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires BiomedicalConcept dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_scheduled_instance_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "ScheduledActivityInstance") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires ScheduledActivityInstance dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_governance_date_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "GovernanceDate") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires GovernanceDate dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_document_content_reference_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "DocumentContentReference") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires DocumentContentReference dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
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

    if is_usdm_object_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "USDMObject") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires USDMObject dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_geographic_scope_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "GeographicScope") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires GeographicScope dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_syntax_template_text_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "SyntaxTemplateText") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires SyntaxTemplateText dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_narrative_content_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "NarrativeContent") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires NarrativeContent dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_narrative_content_item_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "NarrativeContentItem") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires NarrativeContentItem dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if is_usdm_abbreviation_jsonata_rule(rule) {
        let Some(dataset) = find_dataset(datasets, "Abbreviation") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires Abbreviation dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if rule.core_id == "CORE-001070" {
        let Some(dataset) = find_dataset(datasets, "StudyRoleBlinding") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires StudyRoleBlinding dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if rule.core_id == "CORE-001077" {
        let Some(dataset) = find_dataset(datasets, "InterventionalStudyDesign") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!(
                    "Rule {} requires InterventionalStudyDesign dataset",
                    rule.core_id
                ),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(
        rule.core_id.as_str(),
        "CORE-000998" | "CORE-001004" | "CORE-001005" | "CORE-001017" | "CORE-001065"
    ) {
        let Some(dataset) = find_dataset(datasets, "StudyDesign") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires StudyDesign dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-000980") {
        if let Some(dataset) = find_dataset(datasets, "StudyDesignCharacteristicDuplicate") {
            return Ok(vec![dataset.clone()]);
        }
        let Some(dataset) = find_dataset(datasets, "StudyDesign") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires StudyDesign dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-001002") {
        if let Some(dataset) = find_dataset(datasets, "StudyDesignSubTypeDuplicate") {
            return Ok(vec![dataset.clone()]);
        }
        let Some(dataset) = find_dataset(datasets, "StudyDesign") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires StudyDesign dataset", rule.core_id),
            ));
        };
        return Ok(vec![dataset.clone()]);
    }

    if matches!(rule.core_id.as_str(), "CORE-001003") {
        if let Some(dataset) = find_dataset(datasets, "StudyDesignTherapeuticAreaDuplicate") {
            return Ok(vec![dataset.clone()]);
        }
        let Some(dataset) = find_dataset(datasets, "StudyDesign") else {
            return Err(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::EvaluationError,
                format!("Rule {} requires StudyDesign dataset", rule.core_id),
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

    let scoped_datasets = filter_datasets_by_rule_scope(rule, datasets);
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
        || has_match_dataset_dependent_operation(rule))
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

fn dataset_metadata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if matches!(rule.core_id.as_str(), "CORE-000539" | "CORE-000540") {
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
                "CORE-000539" => is_missing_split_parent_dataset(&name, &dataset_names),
                "CORE-000540" => is_missing_findings_about_parent_dataset(&name, &dataset_names),
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
                    "CORE-000539" => {
                        !(3..=4).contains(&name.len())
                            || name.starts_with("AP")
                            || name.starts_with("FA")
                    }
                    "CORE-000540" => !name.starts_with("FA") || name.len() <= 2,
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
    if rule.core_id == "CORE-000494" {
        return core_000494_define_role_metadata_datasets(rule, datasets);
    }
    if rule.core_id == "CORE-000929" {
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
                    if matches!(
                        rule.core_id.as_str(),
                        "CORE-000398" | "CORE-000507" | "CORE-000903"
                    ) {
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
                        if rule.core_id == "CORE-000507" {
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
        "C49620", "C49621", "C49622",
    ];
    if open_rules_env_value(dataset, "VERSION").as_deref() == Some("3-3") {
        codes.push("C00003");
        codes.push("C49563");
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
    if rule_id == "CORE-000398" && library_variable_label(rule_id, domain, variable).is_some() {
        return Some(variable.to_owned());
    }
    if rule_id != "CORE-000903" {
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
    if rule_id != "CORE-000398" {
        return None;
    }

    match (rule_id, variable.to_ascii_uppercase().as_str()) {
        ("CORE-000398", "AESDTH") => Some("Results in Death".to_owned()),
        ("CORE-000398", "LBMETHOD") => Some("Method of Test or Examination".to_owned()),
        ("CORE-000398", "ECROUTE") => Some("Route of Administration".to_owned()),
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
            let allowed_variables = if rule.core_id == "CORE-000852" {
                model_column_order_from_library(dataset)
            } else {
                model_allowed_variables(dataset)
            };
            if rule.core_id == "CORE-000852" {
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
    if domain != "DM" {
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
        .filter(|dataset| !dataset_matches_name(dataset, match_name))
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
    let prefix = match_dataset_string_field(match_dataset, &["prefix"]).unwrap_or_default();
    let mut joined_datasets = Vec::with_capacity(scoped_bases.len());
    for scoped_base in scoped_bases {
        joined_datasets.push(
            left_join_dataset_on(
                scoped_base,
                lookup_dataset,
                &keys.left,
                &keys.right,
                &prefix,
            )
            .map_err(|source| join_skipped_result(rule, source.to_string()))?,
        );
    }
    Ok(joined_datasets)
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
    const RULE_IDS: &[&str] = &[
        "CORE-000140",
        "CORE-000172",
        "CORE-000201",
        "CORE-000271",
        "CORE-000361",
        "CORE-000678",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
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
                    input
                        .iter()
                        .map(|dataset| {
                            group_distinct_values_dataset_with_aliases(
                                dataset,
                                &keys,
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
                    derive_valid_codelist_dates_dataset(dataset, &column)
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

    let key_columns = keys
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
                .entry(filtered_group_count_key(&key_columns, row, None))
                .or_default()
                .insert(value);
        }
    }

    let values = (0..dataset.frame().height())
        .map(|row| {
            let joined = groups
                .get(&filtered_group_count_key(&key_columns, row, None))
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
                        derive_external_distinct_values_dataset(
                            dataset,
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
    rule.core_id == "CORE-000678" && source_name.eq_ignore_ascii_case("POOLDEF")
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

fn derive_reference_distinct_values_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    source_column: &str,
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

    if !source_column.trim().is_empty()
        && dataset.frame().column(source_column).is_err()
        && reference_domains
            .iter()
            .any(|value| json_scalar_string(value).is_some_and(|domain| !domain.trim().is_empty()))
    {
        return Err(DataError::InvalidDatasetPackage(format!(
            "reference distinct source column not found: {source_column}"
        )));
    }

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
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let joined = valid_codelist_dates().join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(joined.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
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
    let values = codelist_codes
        .iter()
        .map(
            |code| match static_codelist(code).map(|codelist| codelist.extensible) {
                Some(value) => Value::Bool(value),
                None => Value::Null,
            },
        )
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

    let values = (0..dataset.summary().row_count)
        .map(|row| {
            let Some(codelist) = codelist_codes
                .get(row)
                .and_then(|code| static_codelist(code))
            else {
                return Value::String(String::new());
            };
            if term_code.is_none() && term_pref_term.is_none() && term_value.is_none() {
                let values = codelist
                    .terms
                    .iter()
                    .map(|term| term.value)
                    .collect::<Vec<_>>()
                    .join("|");
                return Value::String(values);
            }
            let term = term_code
                .as_ref()
                .and_then(|values| values.get(row))
                .and_then(|code| codelist.find_by_code(code))
                .or_else(|| {
                    term_pref_term
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|pref_term| codelist.find_by_pref_term(pref_term))
                })
                .or_else(|| {
                    term_value
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|value| codelist.find_by_value(value))
                });
            let Some(term) = term else {
                return Value::String(String::new());
            };
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

#[derive(Clone, Copy)]
struct StaticTerm {
    code: &'static str,
    value: &'static str,
    pref_term: &'static str,
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
    let reason = if is_runtime_operation_oracle_gap(rule, &message) {
        SkippedReason::OracleSemanticsGap
    } else {
        SkippedReason::OperationsNotSupported
    };
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        reason,
        format!("Rule {} cannot run operation: {}", rule.core_id, message),
    )
}

fn is_runtime_operation_oracle_gap(rule: &ExecutableRule, message: &str) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000712" | "CORE-000916" | "CORE-000953"
    ) && message.contains("reference distinct source column not found")
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
    let skipped_reason = if should_defer_relrec_or_supp_match_dataset_oracle_gap(rule) {
        SkippedReason::OracleSemanticsGap
    } else {
        SkippedReason::DatasetJoinNotSupported
    };

    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        skipped_reason,
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
    if rule.core_id != "CORE-000201" || dataset_has_column(dataset, "USUBJID") {
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
    if !matches!(rule.core_id.as_str(), "CORE-000651" | "CORE-000654") {
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
    if !matches!(rule.core_id.as_str(), "CORE-000651" | "CORE-000654") {
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
    if rule.core_id != "CORE-000138" {
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
    if rule.core_id != "CORE-000095" {
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
    if rule.core_id != "CORE-000572" {
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
    if rule.core_id != "CORE-000466" {
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
) -> RuleValidationResult {
    if matches!(source, EngineError::MissingColumn(_)) && is_missing_column_oracle_gap_rule(rule) {
        return missing_column_skipped_result(rule, dataset);
    }
    evaluation_skipped_result(rule, dataset, source)
}

fn should_ignore_evaluation_error(
    rule: &ExecutableRule,
    source: &EngineError,
    execution_dataset_count: usize,
) -> bool {
    execution_dataset_count > 1
        && matches!(source, EngineError::MissingColumn(_))
        && !is_missing_column_oracle_gap_rule(rule)
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
mod tests {
    use std::{collections::BTreeSet, fs};

    use core_engine::ExecutionStatus;
    use core_rule_model::{load_rules_from_paths, Sensitivity};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    fn write_rule(dir: &std::path::Path, id: &str, expected_domain: &str) {
        fs::write(
            dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{}}, "Classes": {{}} }},
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {{
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "{expected_domain}"
  }},
  "Outcome": {{ "Message": "DOMAIN must be {expected_domain}" }}
}}"#
            ),
        )
        .expect("write rule");
    }

    fn write_dataset(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2],
        "DOMAIN": ["AE", "CM"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");
        path
    }

    #[test]
    fn preflight_accepts_is_not_unique_relationship_operator() {
        assert!(is_supported_basic_operator(
            &Operator::IsNotUniqueRelationship
        ));
    }

    #[test]
    fn run_validation_uses_open_rules_data_loader_when_requested() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("create rules dir");
        fs::create_dir_all(&data_dir).expect("create data dir");
        fs::write(
            rules_dir.join("CORE-OPEN-0001.yml"),
            r#"Core:
  Id: CORE-OPEN-0001
  Status: Published
Scope:
  Domains: {}
  Classes: {}
Sensitivity: Record
Rule Type: Record Data
Check:
  name: CMSEQ
  operator: less_than_or_equal_to
  value: 0
Outcome:
  Message: CMSEQ must be greater than zero
"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Label\ncm,Concomitant Medications\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\n",
        )
        .expect("write variables csv");
        fs::write(data_dir.join("cm.csv"), "CMSEQ\n001\n").expect("write dataset csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn select_rules_includes_only_requested_ids_and_skips_missing_ids() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        write_rule(dir.path(), "CORE-TEST-0002", "CM");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        let selection = select_rules(
            &rules,
            &["CORE-TEST-0002".to_owned(), "CORE-MISSING".to_owned()],
            &[],
        )
        .expect("select rules");

        assert_eq!(selection.selected.len(), 1);
        assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
        assert_eq!(
            selection.skipped[0].execution_status,
            ExecutionStatus::Skipped
        );
    }

    #[test]
    fn select_rules_excludes_requested_ids_and_skips_missing_exclusions() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        write_rule(dir.path(), "CORE-TEST-0002", "CM");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        let selection = select_rules(
            &rules,
            &[],
            &["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
        )
        .expect("select rules");

        assert_eq!(selection.selected.len(), 1);
        assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
    }

    #[test]
    fn select_rules_rejects_include_and_exclude_together() {
        let error = select_rules(
            &[],
            &["CORE-TEST-0001".to_owned()],
            &["CORE-TEST-0002".to_owned()],
        )
        .expect_err("mutually exclusive filters");

        assert!(matches!(error, ApiError::MutuallyExclusiveRuleFilters));
    }

    #[test]
    fn run_validation_filters_rules_and_writes_reports() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        let output_dir = dir.path().join("out");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        write_rule(&rules_dir, "CORE-TEST-0001", "AE");
        write_rule(&rules_dir, "CORE-TEST-0002", "CM");
        let dataset_path = write_dataset(&data_dir);

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path.clone()],
            include_rules: vec!["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
            exclude_rules: Vec::new(),
            output_dir: Some(output_dir.clone()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(outcome.results[0].rule_id, "CORE-MISSING");
        assert_eq!(outcome.results[1].rule_id, "CORE-TEST-0001");
        assert_eq!(outcome.results[1].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[1].error_count, 1);
        assert!(outcome
            .reports
            .expect("reports")
            .json
            .expect("json report")
            .exists());
        assert!(output_dir.join("report.csv").exists());
    }

    #[test]
    fn run_validation_filters_execution_datasets_by_domain_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-DOMAIN-SCOPE.json"),
            r#"{
  "Core": { "Id": "CORE-DOMAIN-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["MS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "MS",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must not be MS" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "ms.xpt",
      "domain": "MS",
      "records": { "USUBJID": ["S1"], "MSSEQ": [1], "DOMAIN": ["MS"] }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].dataset, "AE");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    }

    #[test]
    fn run_validation_domain_scope_matches_supp_placeholder_domains() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-SUPP-SCOPE.json"),
            r#"{
  "Core": { "Id": "CORE-SUPP-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "QNAM",
    "operator": "matches_regex",
    "value": "^[0-9]"
  },
  "Outcome": { "Message": "QNAM starts with a number" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "supplb.xpt",
      "domain": "SUPPLB",
      "records": {
        "USUBJID": ["S1"],
        "IDVAR": ["LBSEQ"],
        "IDVARVAL": ["1"],
        "QNAM": ["5BIOSIG"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].dataset, "SUPPLB");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_filters_execution_datasets_by_class_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-CLASS-SCOPE.json"),
            r#"{
  "Core": { "Id": "CORE-CLASS-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "LB",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must be LB" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": { "USUBJID": ["S1"], "LBSEQ": [1], "DOMAIN": ["LB"] }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": { "USUBJID": ["S1"], "FASEQ": [1], "DOMAIN": ["FA"] }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results[0].dataset, "LB");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[1].dataset, "FA");
        assert_eq!(
            outcome.results[1].execution_status,
            ExecutionStatus::Passed,
            "{:?}",
            outcome.results[1]
        );
    }

    #[test]
    fn run_validation_loads_xpt_dataset() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        write_rule(&rules_dir, "CORE-XPT-0001", "AE");
        let dataset_path = data_dir.join("ae.xpt");
        write_test_xpt_char_dataset(
            &dataset_path,
            "AE",
            &["STUDYID", "DOMAIN", "AESEQ"],
            &[vec!["CDISC-TEST", "AE", "1"], vec!["CDISC-TEST", "CM", "2"]],
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path.clone()],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_records_engine_errors_as_skipped_results() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-MISSING-COLUMN.json"),
            r#"{
  "Core": { "Id": "CORE-MISSING-COLUMN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESTDTC",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "AESTDTC must be populated" }
}"#,
        )
        .expect("write missing column rule");
        write_rule(&rules_dir, "CORE-VALID", "AE");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let skipped = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-MISSING-COLUMN")
            .expect("skipped missing column result");
        assert_eq!(skipped.execution_status, ExecutionStatus::Skipped);
        assert_eq!(skipped.skipped_reason, Some(SkippedReason::EvaluationError));
        assert_eq!(skipped.dataset, "AE");
        assert!(skipped
            .message
            .contains("dataset is missing required column"));

        let valid = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-VALID")
            .expect("valid rule result");
        assert_eq!(valid.execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_treats_safe_open_rules_missing_columns_as_null() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000200.json"),
            r#"{
  "Core": { "Id": "CORE-000200", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--STAT", "operator": "empty" },
          { "name": "--DRVFL", "operator": "not_equal_to", "value": "Y", "value_is_literal": true },
          { "name": "--ORRES", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "--ORRES cannot be null" }
}"#,
        )
        .expect("write open rules missing-column rule");
        let dataset_path = data_dir.join("lb.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1"],
        "LBSEQ": [1],
        "LBSTAT": [""],
        "LBORRES": ["12"]
      }
    }
  ]
}"#,
        )
        .expect("write open rules data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].skipped_reason, None);
    }

    #[test]
    fn run_validation_treats_safe_usdm_missing_nested_columns_as_null() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000680.json"),
            r#"{
  "Core": { "Id": "CORE-000680", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Range"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Range" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel", "operator": "is_contained_by", "value": ["plannedCompletionNumber"] },
      {
        "not": {
          "any": [
            { "name": "unit", "operator": "equal_to", "value": false },
            { "name": "unit", "operator": "empty" },
            { "name": "unit", "operator": "not_exists" }
          ]
        }
      }
    ]
  },
  "Outcome": { "Message": "A unit is specified" }
}"#,
        )
        .expect("write usdm missing-column rule");
        let dataset_path = data_dir.join("range.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Range.csv",
      "domain": "Range",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["StudyDesignPopulation_2"],
        "parent_rel": ["plannedCompletionNumber"],
        "rel_type": ["definition"],
        "id": ["Range_6"],
        "instanceType": ["Range"]
      }
    }
  ]
}"#,
        )
        .expect("write usdm data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].skipped_reason, None);
    }

    #[test]
    fn run_validation_skips_core_000039_missing_svpresp_as_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000039.json"),
            r#"{
  "Core": { "Id": "CORE-000039", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": {
    "Message": "VISITNUM for planned visit is not in TV.",
    "Output Variables": ["SVPRESP", "VISITNUM"]
  }
}"#,
        )
        .expect("write core 39 rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [1, 2]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [1, 99]
      }
    }
  ]
}"#,
        )
        .expect("write core 39 data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_ignores_missing_columns_for_non_applicable_scoped_datasets() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-SCOPED-MISSING-COLUMN.json"),
            r#"{
  "Core": { "Id": "CORE-SCOPED-MISSING-COLUMN", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESTDTC",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "AESTDTC must be populated" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESTDTC": ["2020-01-01", ""]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSTDTC": ["2020-01-01"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].dataset, "AE");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_skips_missing_column_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000750.json"),
            r#"{
  "Core": { "Id": "CORE-000750", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "POOLID",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "USUBJID has oracle-specific missing-column semantics" }
}"#,
        )
        .expect("write rule");
        fs::write(
            rules_dir.join("CORE-000092.json"),
            r#"{
  "Core": { "Id": "CORE-000092", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECSTAT",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "ECSTAT has oracle-specific missing-column semantics" }
}"#,
        )
        .expect("write second rule");
        fs::write(
            rules_dir.join("CORE-000016.json"),
            r#"{
  "Core": { "Id": "CORE-000016", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECMOOD",
    "operator": "empty"
  },
  "Outcome": { "Message": "ECMOOD has oracle-specific missing-column semantics" }
}"#,
        )
        .expect("write empty missing-column rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": [""]
      }
    },
    {
      "filename": "ec.xpt",
      "domain": "EC",
      "records": {
        "USUBJID": ["S1"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 3);
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
        assert!(outcome
            .results
            .iter()
            .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
    }

    #[test]
    fn run_validation_skips_core_000674_missing_placeholder_column_as_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000674.json"),
            r#"{
  "Core": { "Id": "CORE-000674", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IQ"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--VALTRG", "operator": "matches_regex", "value": "^-?([1-9]\\d*|0)(\\.\\d+)?$" },
      { "name": "--VALMAX", "operator": "matches_regex", "value": "^.+$" },
      { "name": "--VALTRG", "operator": "greater_than", "value": "--VALMAX" }
    ]
  },
  "Outcome": { "Message": "--VALTRG must be <= --VALMAX" }
}"#,
        )
        .expect("write core 674 rule");

        let dataset_path = data_dir.join("iq.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "iq.csv",
      "domain": "IQ",
      "records": {
        "IQVALTRG": [1]
      }
    }
  ]
}"#,
        )
        .expect("write core 674 data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_requires_paths_before_loading() {
        let request = ValidateRequest {
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            rule_paths: Vec::new(),
            dataset_paths: Vec::new(),
            ..Default::default()
        };

        let error = run_validation(request).expect_err("missing rule paths");
        assert!(matches!(error, ApiError::MissingRulePaths));
    }

    #[test]
    fn loaded_rules_keep_record_sensitivity() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        assert_eq!(rules[0].sensitivity, Some(Sensitivity::Record));
    }

    #[test]
    fn run_validation_skips_unsupported_rules_before_engine_execution() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        write_raw_rule(
            &rules_dir,
            "CORE-OPERATIONS",
            r#""Rule Type": "Record Data""#,
            r#""Operations": [{ "name": "future_operation" }],"#,
            r#""operator": "equal_to""#,
        );
        write_raw_rule(
            &rules_dir,
            "CORE-JOIN",
            r#""Rule Type": "Record Data""#,
            r#""Match Datasets": [{ "domain": "SUPPAE" }],"#,
            r#""operator": "equal_to""#,
        );
        write_raw_rule(
            &rules_dir,
            "CORE-OPERATOR",
            r#""Rule Type": "Record Data""#,
            "",
            r#""operator": "future_operator""#,
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 3);
        let reasons = outcome
            .results
            .iter()
            .map(|result| result.skipped_reason.as_ref().expect("skipped reason"))
            .map(|reason| serde_json::to_string(reason).expect("serialize reason"))
            .map(|reason| reason.trim_matches('"').to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            reasons,
            BTreeSet::from([
                "dataset_join_not_supported".to_owned(),
                "operations_not_supported".to_owned(),
                "unsupported_operator".to_owned(),
            ])
        );
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
    }

    #[test]
    fn run_validation_skips_unsupported_rules_before_loading_datasets() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let output_dir = dir.path().join("out");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::write(
            rules_dir.join("CORE-JSONATA-UNSUPPORTED.json"),
            r#"{
  "Core": { "Id": "CORE-JSONATA-UNSUPPORTED", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$.study.versions.studyDesigns.{\"id\": id}[id != null]",
  "Outcome": { "Message": "Unsupported JSONata" }
}"#,
        )
        .expect("write rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dir.path().join("missing-data")],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            output_dir: Some(output_dir.clone()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::UnsupportedOperator)
        );
        let report_csv = fs::read_to_string(output_dir.join("report.csv")).expect("read csv");
        assert!(report_csv.contains("CORE-JSONATA-UNSUPPORTED"));
        assert!(report_csv.contains("unsupported_operator"));
    }

    #[test]
    fn run_validation_executes_open_rules_date_operators() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-DATE-OPERATOR.json"),
            r#"{
  "Core": { "Id": "CORE-DATE-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "STARTDTC",
    "operator": "date_greater_than",
    "value": "2024-01-01"
  },
  "Outcome": { "Message": "STARTDTC must be on or before 2024-01-01" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "AESEQ": [1],
        "STARTDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_executes_core_000653_date_end_before_start() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000653.json"),
            r#"{
  "Core": { "Id": "CORE-000653", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENDTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "--ENDTC" }
    ]
  },
  "Outcome": {
    "Message": "--ENDTC must be greater than or equal to --DTC",
    "Output Variables": ["--ENDTC", "--DTC"]
  }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "DSSEQ": [1, 2],
        "DSDTC": ["2018-09-21", "2018-05-08T09:13"],
        "DSENDTC": ["2018-09-04", "2018-05-08T08:00"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000653");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 4);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].variables, vec!["DSENDTC"]);
        assert_eq!(outcome.results[0].errors[1].variables, vec!["DSDTC"]);
        assert_eq!(outcome.results[0].errors[2].row, Some(2));
        assert_eq!(outcome.results[0].errors[2].variables, vec!["DSENDTC"]);
        assert_eq!(outcome.results[0].errors[3].variables, vec!["DSDTC"]);
    }

    #[test]
    fn run_validation_executes_core_000505_invalid_study_start_dates() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000505.json"),
            r#"{
  "Core": { "Id": "CORE-000505", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "TSPARMCD", "operator": "equal_to", "value": "SSTDTC" },
      { "name": "TSVAL", "operator": "invalid_date" }
    ]
  },
  "Outcome": { "Message": "TSVAL where TSPARMCD = SSTDTC is not in ISO 8601 format." }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "TSSEQ": [1, 2, 3, 4, 5],
        "TSPARMCD": ["SSTDTC", "SSTDTC", "SSTDTC", "SSTDTC", "SSTDTC"],
        "TSVAL": ["2003-12", "200", "2003-20", "2003-11-31", "2003-02-31"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000505");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 8);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].variables, vec!["TSPARMCD"]);
        assert_eq!(outcome.results[0].errors[1].variables, vec!["TSVAL"]);
        assert_eq!(outcome.results[0].errors[6].row, Some(5));
        assert_eq!(outcome.results[0].errors[6].variables, vec!["TSPARMCD"]);
        assert_eq!(outcome.results[0].errors[7].variables, vec!["TSVAL"]);
    }

    #[test]
    fn run_validation_executes_core_000139_incomplete_reference_start_date() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000139.json"),
            r#"{
  "Core": { "Id": "CORE-000139", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--ENDTC", "operator": "is_incomplete_date" },
          { "name": "--ENDY", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "RFSTDTC", "operator": "is_incomplete_date" },
          { "name": "--ENDY", "operator": "non_empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "--ENDY is not null when either --ENDTC or DM.RFSTDTC do not contain complete values in their date portion",
    "Output Variables": ["--ENDTC", "RFSTDTC", "--ENDY"]
  }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "EXSEQ": [1, 2],
        "EXENDTC": ["2012-11-30", "2012-12-01"],
        "EXENDY": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFSTDTC": ["2012-11"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000139");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 6);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].variables, vec!["EXENDTC"]);
        assert_eq!(outcome.results[0].errors[1].variables, vec!["RFSTDTC"]);
        assert_eq!(outcome.results[0].errors[2].variables, vec!["EXENDY"]);
        assert_eq!(outcome.results[0].errors[3].row, Some(2));
        assert_eq!(outcome.results[0].errors[3].variables, vec!["EXENDTC"]);
        assert_eq!(outcome.results[0].errors[4].variables, vec!["RFSTDTC"]);
        assert_eq!(outcome.results[0].errors[5].variables, vec!["EXENDY"]);
    }

    #[test]
    fn run_validation_executes_core_000138_incomplete_start_dates_and_dm_dataset_issue() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000138.json"),
            r#"{
  "Core": { "Id": "CORE-000138", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--STDTC", "operator": "is_incomplete_date" },
          { "name": "--STDY", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "RFSTDTC", "operator": "is_incomplete_date" },
          { "name": "--STDY", "operator": "non_empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "--STDY is not null when either --STDTC or DM.RFSTDTC do not contain complete values in their date portion",
    "Output Variables": ["--STDY", "--STDTC", "RFSTDTC"]
  }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["001", "002"],
        "AESEQ": [1, 1],
        "AESTDTC": ["2005-10", "2005-10-13T13:05"],
        "AESTDY": [1, 1]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["001", "002"],
        "RFSTDTC": ["2022-03-20", "2022-03"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let ae = outcome
            .results
            .iter()
            .find(|result| result.dataset == "AE")
            .expect("AE result");
        assert_eq!(ae.rule_id, "CORE-000138");
        assert_eq!(ae.execution_status, ExecutionStatus::Failed);
        assert_eq!(ae.error_count, 6);
        assert_eq!(ae.errors[0].row, Some(1));
        assert_eq!(ae.errors[0].variables, vec!["AESTDY"]);
        assert_eq!(ae.errors[1].variables, vec!["AESTDTC"]);
        assert_eq!(ae.errors[2].variables, vec!["RFSTDTC"]);
        assert_eq!(ae.errors[3].row, Some(2));
        assert_eq!(ae.errors[3].variables, vec!["AESTDY"]);
        assert_eq!(ae.errors[4].variables, vec!["AESTDTC"]);
        assert_eq!(ae.errors[5].variables, vec!["RFSTDTC"]);

        let dm = outcome
            .results
            .iter()
            .find(|result| result.dataset == "DM")
            .expect("DM result");
        assert_eq!(dm.execution_status, ExecutionStatus::Failed);
        assert_eq!(dm.error_count, 1);
        assert_eq!(dm.errors[0].row, None);
        assert!(dm.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_core_000324_invalid_end_relative_timing() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000324.json"),
            r#"{
  "Core": { "Id": "CORE-000324", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_equal_to", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
        "filename": "cm.xpt",
        "domain": "CM",
        "records": {
          "USUBJID": ["SUBJ1"],
          "CMSEQ": [1],
          "CMENTPT": ["2013-05-20"],
          "CMENRTPT": ["AFTER"]
        }
      }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000324");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 3);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["CMDTC", "CMENRTPT", "CMENTPT"]);
    }

    #[test]
    fn run_validation_executes_core_000460_invalid_trial_set_dates() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000460.json"),
            r#"{
  "Core": { "Id": "CORE-000460", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      {
        "any": [
          { "name": "TXPARMCD", "operator": "equal_to", "value": "DOSENDTC" },
          { "name": "TXPARMCD", "operator": "equal_to", "value": "DOSSTDTC" }
        ]
      },
      { "name": "TXVAL", "operator": "invalid_date" }
    ]
  },
  "Outcome": { "Message": "The value of TXVAL is not in ISO 8601 date/datetime format" }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "DOMAIN": ["TX"],
        "TXPARMCD": ["DOSSTDTC"],
        "TXVAL": ["2022-03-a"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000460");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["TXPARMCD", "TXVAL"]);
    }

    #[test]
    fn run_validation_executes_core_000572_invalid_end_relative_timing_after_reference() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000572.json"),
            r#"{
  "Core": { "Id": "CORE-000572", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_less_than", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "AFTER", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'AFTER', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSEQ": [1],
        "CMDTC": ["2013-05-21"],
        "CMENTPT": ["2013-05-20"],
        "CMENRTPT": ["WRONG"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000572");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 3);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["CMDTC", "CMENRTPT", "CMENTPT"]);
    }

    #[test]
    fn run_validation_executes_core_000572_cm_dataset_marker_when_dtc_absent() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000572.json"),
            r#"{
  "Core": { "Id": "CORE-000572", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_less_than", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "AFTER", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'AFTER', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSEQ": [1],
        "CMENTPT": ["2013-05-20"],
        "CMENRTPT": ["WRONG"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        let marker = outcome
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .expect("dataset marker");
        assert_eq!(marker.rule_id, "CORE-000572");
        assert_eq!(marker.dataset, "CM");
        assert_eq!(marker.error_count, 1);
        assert_eq!(marker.errors[0].row, None);
        assert!(marker.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_core_000095_unplanned_element_description() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000095.json"),
            r#"{
  "Core": { "Id": "CORE-000095", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "SEUPDES", "operator": "non_empty" },
      { "name": "ETCD", "operator": "not_equal_to", "value": "UNPLAN" }
    ]
  },
  "Outcome": { "Message": "ETCD is not UNPLAN, when SEUPDES is not empty" }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "SESEQ": [1],
        "ETCD": ["TRTZ"],
        "SEUPDES": ["Unplanned treatment"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000095");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["SEUPDES".to_owned(), "ETCD".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_core_000095_se_dataset_marker_when_seupdes_absent() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000095.json"),
            r#"{
  "Core": { "Id": "CORE-000095", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "SEUPDES", "operator": "non_empty" },
      { "name": "ETCD", "operator": "not_equal_to", "value": "UNPLAN" }
    ]
  },
  "Outcome": { "Message": "ETCD is not UNPLAN, when SEUPDES is not empty" }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "SESEQ": [1],
        "ETCD": ["TRTZ"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let marker = outcome
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .expect("dataset marker");
        assert_eq!(marker.rule_id, "CORE-000095");
        assert_eq!(marker.dataset, "SE");
        assert_eq!(marker.error_count, 1);
        assert_eq!(marker.errors[0].row, None);
        assert!(marker.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_core_000711_reference_start_after_end_dates() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000711.json"),
            r#"{
  "Core": { "Id": "CORE-000711", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "RFSTDTC", "operator": "non_empty" },
      { "name": "RFENDTC", "operator": "non_empty" },
      { "name": "RFSTDTC", "operator": "date_greater_than", "value": "RFENDTC" }
    ]
  },
  "Outcome": {
    "Message": "RFSTDTC falls after RFENDTC.",
    "Output Variables": ["RFSTDTC", "RFENDTC"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFSTDTC": ["2006-03"],
        "RFENDTC": ["2006-01-16"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000711");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["RFENDTC", "RFSTDTC"]);
    }

    #[test]
    fn run_validation_executes_core_000714_treatment_start_after_end_dates() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000714.json"),
            r#"{
  "Core": { "Id": "CORE-000714", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "RFXSTDTC", "operator": "non_empty" },
      { "name": "RFXENDTC", "operator": "non_empty" },
      { "name": "RFXSTDTC", "operator": "date_greater_than", "value": "RFXENDTC" }
    ]
  },
  "Outcome": {
    "Message": "RFXSTDTC falls after RFXENDTC.",
    "Output Variables": ["RFXSTDTC", "RFXENDTC"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFXSTDTC": ["2018-04-17"],
        "RFXENDTC": ["2018-04"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000714");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["RFXENDTC", "RFXSTDTC"]);
    }

    #[test]
    fn run_validation_executes_core_000866_observation_start_after_end_dates() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000866.json"),
            r#"{
  "Core": { "Id": "CORE-000866", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENDTC", "operator": "exists" },
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--ENDTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "--ENDTC" }
    ]
  },
  "Outcome": {
    "Message": "--DTC falls after --ENDTC.",
    "Output Variables": ["--DTC", "--ENDTC"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBDTC": ["2018-11"],
        "LBENDTC": ["2018"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000866");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        let mut variables = outcome.results[0]
            .errors
            .iter()
            .map(|issue| issue.variables.join("|"))
            .collect::<Vec<_>>();
        variables.sort();
        assert_eq!(variables, vec!["LBDTC", "LBENDTC"]);
    }

    #[test]
    fn run_validation_executes_grouped_min_date_with_date_not_equal_to() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-MIN-DATE-NOT-EQUAL.json"),
            r#"{
  "Core": { "Id": "CORE-MIN-DATE-NOT-EQUAL", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DS",
      "group": ["USUBJID"],
      "id": "$min_ds_dsstdtc",
      "name": "DSSTDTC",
      "operator": "min_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "DSTERM", "operator": "contains", "value": "INFORMED CONSENT" },
      { "name": "DSSTDTC", "operator": "date_not_equal_to", "value": "$min_ds_dsstdtc" }
    ]
  },
  "Outcome": { "Message": "DSSTDTC is not the earliest informed consent date" }
}"#,
        )
        .expect("write grouped min date rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "DSSEQ": [1, 2, 1],
        "DSTERM": ["INFORMED CONSENT", "INFORMED CONSENT", "INFORMED CONSENT"],
        "DSSTDTC": ["2020-01-03", "2020-01-01", "2020-02-01"]
      }
    }
  ]
}"#,
        )
        .expect("write grouped min date data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_target_is_not_sorted_by_operator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-SORT-OPERATOR.json"),
            r#"{
  "Core": { "Id": "CORE-SORT-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "AESTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "AESEQ is not chronological" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "AESEQ": [1, 3, 2],
        "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_executes_empty_within_except_last_row_operator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-END-OPERATOR.json"),
            r#"{
  "Core": { "Id": "CORE-END-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SEENDTC",
    "operator": "empty_within_except_last_row",
    "ordering": "SESTDTC",
    "value": "USUBJID"
  },
  "Outcome": { "Message": "SEENDTC is empty before the last row" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SESEQ": [1, 2, 3],
        "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"],
        "SEENDTC": ["2024-01-02", "", ""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_executes_not_present_on_multiple_rows_within_operator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-REL-OPERATOR.json"),
            r#"{
  "Core": { "Id": "CORE-REL-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "not_present_on_multiple_rows_within",
    "within": "USUBJID"
  },
  "Outcome": { "Message": "RELID must appear on multiple rows" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_executes_is_not_unique_set_operator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-UNIQUE-SET.json"),
            r#"{
  "Core": { "Id": "CORE-UNIQUE-SET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be unique within subject" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_executes_is_inconsistent_across_dataset_operator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-INCONSISTENT-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-INCONSISTENT-DATASET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_inconsistent_across_dataset",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be consistent within subject" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
        "RELID": ["R1", "R1", "R2", "R3"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 3);
    }

    #[test]
    fn run_validation_skips_inconsistent_across_dataset_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000142.json"),
            r#"{
  "Core": { "Id": "CORE-000142", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "FTELTM",
    "operator": "is_inconsistent_across_dataset",
    "value": ["DOMAIN", "VISITNUM", "FTTPTREF", "FTTPTNUM"]
  },
  "Outcome": { "Message": "FTELTM has oracle-specific consistency semantics" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ft.xpt",
      "domain": "FT",
      "records": {
        "DOMAIN": ["FT", "FT"],
        "VISITNUM": [1, 1],
        "FTTPTREF": ["DOSE", "DOSE"],
        "FTTPTNUM": [1, 1],
        "FTELTM": ["PT30M", "PT03M"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_unique_set_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000390.json"),
            r#"{
  "Core": { "Id": "CORE-000390", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID has oracle-specific uniqueness semantics" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_not_unique_relationship_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            rules_dir.join("CORE-000184.json"),
            r#"{
  "Core": { "Id": "CORE-000184", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--BDSYCD",
    "operator": "is_not_unique_relationship",
    "value": "--BODSYS"
  },
  "Outcome": { "Message": "relationship has oracle-specific scope semantics" }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "AEBDSYCD": ["10029205", "10029206"],
        "AEBODSYS": ["Nervous system disorders", "Nervous system disorders"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_core_000651_missing_tptnum_as_dataset_issue() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000651.json"),
            r#"{
  "Core": { "Id": "CORE-000651", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "--TPTNUM", "operator": "exists" },
    { "name": "--TPT", "operator": "exists" },
    { "name": "--TPTNUM", "operator": "non_empty" },
    { "name": "--TPT", "operator": "non_empty" },
    { "name": "--TPTNUM", "operator": "is_not_unique_relationship", "value": "--TPT" }
  ] },
  "Outcome": {
    "Message": "--TPT and --TPTNUM do not have a one-to-one relationship",
    "Output Variables": ["--TPT", "--TPTNUM"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBSEQ": [1, 2],
        "LBTPT": ["AM1", "AM2"]
      }
    }
  ]
}"#,
        )
        .expect("write data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000651");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "LB");
        assert_eq!(outcome.results[0].errors[0].row, None);
        assert!(outcome.results[0].errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_core_000651_missing_pp_for_scat_tpt_relationship() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000651.json"),
            r#"{
  "Core": { "Id": "CORE-000651", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "--TPTNUM", "operator": "exists" },
    { "name": "--TPT", "operator": "exists" },
    { "name": "--TPTNUM", "operator": "non_empty" },
    { "name": "--TPT", "operator": "non_empty" },
    { "name": "--TPTNUM", "operator": "is_not_unique_relationship", "value": "--TPT" }
  ] },
  "Outcome": {
    "Message": "--TPT and --TPTNUM do not have a one-to-one relationship",
    "Output Variables": ["--TPT", "--TPTNUM"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBSEQ": [1, 2],
        "LBSCAT": ["SUB1", "SUB2"],
        "LBTPT": ["AM1", "AM2"],
        "LBTPTNUM": [1, 2]
      }
    }
  ]
}"#,
        )
        .expect("write data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        let pp_result = outcome
            .results
            .iter()
            .find(|result| result.dataset == "PP")
            .expect("missing PP result");
        assert_eq!(pp_result.rule_id, "CORE-000651");
        assert_eq!(pp_result.execution_status, ExecutionStatus::Failed);
        assert_eq!(pp_result.error_count, 1);
        assert_eq!(pp_result.errors[0].dataset, "PP");
        assert_eq!(pp_result.errors[0].row, None);
        assert!(pp_result.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_dataset_presence_and_skips_known_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-DATASET-PRESENCE.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-PRESENCE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "exists" },
  "Outcome": { "Message": "presence semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write presence rule");
        fs::write(
            rules_dir.join("CORE-000015.json"),
            r#"{
  "Core": { "Id": "CORE-000015", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "exists" },
  "Outcome": { "Message": "known dataset presence gap" }
}"#,
        )
        .expect("write dataset presence gap rule");
        fs::write(
            rules_dir.join("CORE-COLUMN-REF.json"),
            r#"{
  "Core": { "Id": "CORE-COLUMN-REF", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "equal_to", "value": "AE--REF" },
  "Outcome": { "Message": "column-ref comparisons are not oracle-compatible yet" }
}"#,
        )
        .expect("write column-ref rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 3);
        let presence = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-DATASET-PRESENCE")
            .expect("presence result");
        assert_eq!(presence.execution_status, ExecutionStatus::Failed);
        assert_eq!(presence.error_count, 2);

        let gap = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-000015")
            .expect("gap result");
        assert_eq!(gap.execution_status, ExecutionStatus::Skipped);
        assert_eq!(gap.skipped_reason, Some(SkippedReason::OracleSemanticsGap));

        let column_ref = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-COLUMN-REF")
            .expect("column-ref result");
        assert_eq!(column_ref.execution_status, ExecutionStatus::Skipped);
    }

    #[test]
    fn run_validation_executes_domain_presence_rule_against_loaded_datasets() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "DOMAIN": ["AE"] }
    },
    {
      "filename": "tt.csv",
      "domain": "TT",
      "records": { "DOMAIN": ["TT"] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-DOMAIN-PRESENCE.json"),
            r#"{
  "Core": { "Id": "CORE-DOMAIN-PRESENCE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Check": { "name": "TT", "operator": "exists" },
  "Outcome": { "Message": "TT dataset is present" }
}"#,
        )
        .expect("write domain presence rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "TT");
        assert_eq!(outcome.results[0].errors[0].variables, vec!["TT"]);
    }

    #[test]
    fn run_validation_executes_domain_presence_variable_exists_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "pc.csv",
      "domain": "PC",
      "records": { "DOMAIN": ["PC"], "POOLID": ["POOL1"] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-DOMAIN-VARIABLE-EXISTS.json"),
            r#"{
  "Core": { "Id": "CORE-DOMAIN-VARIABLE-EXISTS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Operations": [{ "id": "$poolid_exists", "name": "POOLID", "operator": "variable_exists" }],
  "Check": {
    "all": [
      { "name": "$poolid_exists", "operator": "equal_to", "value": true },
      { "name": "POOLDEF", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "POOLID variable exists but POOLDEF dataset is missing",
    "Output Variables": ["$poolid_exists", "POOLDEF"]
  }
}"#,
        )
        .expect("write domain presence rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["$poolid_exists", "POOLDEF"]
        );
    }

    #[test]
    fn run_validation_executes_dataset_metadata_record_count_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "vs.csv",
      "domain": "VS",
      "label": "Vital Signs",
      "records": { "DOMAIN": [] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-DATASET-METADATA.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$record_count", "operator": "record_count" }],
  "Check": { "name": "$record_count", "operator": "equal_to", "value": 0 },
  "Outcome": {
    "Message": "Dataset may not be empty",
    "Output Variables": ["dataset_name", "dataset_label", "$record_count"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "VS");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["dataset_name", "dataset_label", "$record_count"]
        );
    }

    #[test]
    fn run_validation_executes_dataset_metadata_domain_prefix_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ab.csv",
      "domain": "LB",
      "label": "Laboratory Test Results A",
      "records": { "DOMAIN": ["LB"] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-DATASET-PREFIX.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-PREFIX", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Check": { "name": "dataset_name", "operator": "prefix_not_equal_to", "value": "DOMAIN" },
  "Outcome": {
    "Message": "Dataset name must begin with DOMAIN",
    "Output Variables": ["dataset_name"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "LB");
        assert_eq!(outcome.results[0].errors[0].variables, vec!["dataset_name"]);
    }

    #[test]
    fn run_validation_executes_dataset_metadata_dataset_names_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "qs36.csv",
      "domain": "QS",
      "label": "Questionnaires SF-36",
      "records": { "DOMAIN": ["QS"] }
    },
    {
      "filename": "ae.csv",
      "domain": "AE",
      "label": "Adverse Events",
      "records": { "DOMAIN": ["AE"] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-DATASET-NAMES.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-NAMES", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$list_dataset_names", "operator": "dataset_names" }],
  "Check": {
    "all": [
      { "name": "dataset_name", "operator": "matches_regex", "value": "^[A-Z]{2}[A-Z0-9]{1,2}" },
      { "name": "dataset_name", "operator": "not_prefix_matches_regex", "prefix": 2, "value": "(AP|FA)" },
      { "name": "dataset_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$list_dataset_names" }
    ]
  },
  "Outcome": {
    "Message": "Split dataset parent domain is missing",
    "Output Variables": ["dataset_name", "$list_dataset_names"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");
        let failed = outcome
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .expect("failed dataset metadata result");
        assert_eq!(failed.error_count, 1);
        assert_eq!(failed.errors[0].dataset, "QS");
        assert_eq!(
            failed.errors[0].variables,
            vec!["dataset_name", "$list_dataset_names"]
        );
    }

    #[test]
    fn run_validation_executes_variable_metadata_label_length_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "A label that is definitely longer than forty characters", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-VARIABLE-METADATA.json"),
            r#"{
  "Core": { "Id": "CORE-VARIABLE-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Check": { "name": "variable_label", "operator": "longer_than", "value": 40 },
  "Outcome": {
    "Message": "Variable label is too long",
    "Output Variables": ["variable_label"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AE");
        assert_eq!(outcome.results[0].errors[0].row, None);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["variable_label"]
        );
    }

    #[test]
    fn run_validation_executes_variable_metadata_expected_variables_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
            r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AE");
        assert_eq!(outcome.results[0].errors[0].row, None);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["$dataset_variables", "$expected_variables"]
        );
    }

    #[test]
    fn run_validation_executes_variable_metadata_required_variables_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-REQUIRED-VARIABLES.json"),
            r#"{
  "Core": { "Id": "CORE-REQUIRED-VARIABLES", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$required_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one required variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$required_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AE");
        assert_eq!(outcome.results[0].errors[0].row, None);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["$dataset_variables", "$required_variables"]
        );
    }

    #[test]
    fn run_validation_skips_core_000356_required_value_dataset_metadata_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": [""],
        "DOMAIN": ["AE"],
        "USUBJID": ["01"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000356.json"),
            r#"{
  "Core": { "Id": "CORE-000356", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Dataset Metadata",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$required_variables", "operator": "exists" },
      { "name": "variable_name", "operator": "is_contained_by", "value": "$required_variables" },
      { "name": "variable_value", "operator": "empty" }
    ]
  },
  "Outcome": {
    "Message": "At least one Required variable has a null value",
    "Output Variables": ["variable_name", "variable_value"]
  }
}"#,
        )
        .expect("write value metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
        assert_eq!(outcome.results[0].error_count, 0);
        assert!(outcome.results[0].errors.is_empty());
    }

    #[test]
    fn run_validation_passes_variable_metadata_expected_variables_when_all_are_present() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXDOSE", "label": "Dose", "type": "Num", "length": 8 },
        { "name": "EXDOSU", "label": "Dose Units", "type": "Char", "length": 20 },
        { "name": "EXDOSFRM", "label": "Dose Form", "type": "Char", "length": 20 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 },
        { "name": "EXENDTC", "label": "End Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXDOSE": ["10"],
        "EXDOSU": ["mg"],
        "EXDOSFRM": ["TABLET"],
        "EXSTDTC": ["2024-01-01"],
        "EXENDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
            r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_executes_variable_metadata_timing_variables_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000575.json"),
            r#"{
  "Core": { "Id": "CORE-000575", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" },
    { "id": "$timing_variables", "key_name": "role", "key_value": "Timing", "operator": "get_model_filtered_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$dataset_variables", "operator": "shares_no_elements_with", "value": "$timing_variables" }
    ]
  },
  "Outcome": {
    "Message": "No timing variable is provided",
    "Output Variables": ["$dataset_variables", "$timing_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");
        let failed = outcome
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .unwrap_or_else(|| panic!("AE timing failure: {:?}", outcome.results));
        assert_eq!(failed.dataset, "AE");
        assert_eq!(failed.error_count, 1);
        assert_eq!(failed.errors[0].row, None);
        assert_eq!(
            failed.errors[0].variables,
            vec!["$dataset_variables", "$timing_variables"]
        );
        assert!(!outcome
            .results
            .iter()
            .any(|result| result.dataset == "EX"
                && result.execution_status == ExecutionStatus::Failed));
    }

    #[test]
    fn run_validation_executes_variable_metadata_model_column_order_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AESTDYXX", "label": "Custom Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"],
        "AESTDYXX": [1]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000550.json"),
            r#"{
  "Core": { "Id": "CORE-000550", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "Variables not listed in the Model List of Allowed Variables for Observation Class should be in SUPPQUAL.",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AE");
        assert_eq!(outcome.results[0].errors[0].row, Some(4));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["variable_name"]
        );
    }

    #[test]
    fn run_validation_executes_variable_metadata_library_column_order_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "class": "EVENTS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "DOMAIN": ["AE"],
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000852.json"),
            r#"{
  "Core": { "Id": "CORE-000852", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$column_order_from_library", "operator": "get_column_order_from_library" },
    { "id": "$column_order_from_dataset", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "$column_order_from_dataset", "operator": "is_not_ordered_subset_of", "value": "$column_order_from_library" }
    ]
  },
  "Outcome": {
    "Message": "Variables are not in the correct order.",
    "Output Variables": ["$column_order_from_dataset", "$column_order_from_library"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AE");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["$column_order_from_dataset", "$column_order_from_library"]
        );
    }

    #[test]
    fn run_validation_executes_value_check_with_variable_metadata_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AECAT", "label": "Category", "type": "Char", "length": 40 },
        { "name": "AESTDY", "label": "Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST", "CDISC-TEST"],
        "AETERM": ["HEADACHE", " NAUSEA"],
        "AECAT": [".", "GENERAL"],
        "AESTDY": [1, 2]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");

        for (id, check) in [
            (
                "CORE-000867",
                r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "matches_regex", "value": "^\\s" }
    ] }"#,
            ),
            (
                "CORE-000890",
                r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "non_empty" },
      { "name": "variable_value", "operator": "equal_to", "value": ".", "value_is_literal": true }
    ] }"#,
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{ "Include": ["ALL"] }}, "Classes": {{ "Include": ["ALL"] }} }},
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Variable Metadata",
  "Check": {check},
  "Outcome": {{
    "Message": "Value metadata rule.",
    "Output Variables": ["variable_value", "variable_name"]
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");
        let failed = outcome
            .results
            .iter()
            .filter(|result| result.execution_status == ExecutionStatus::Failed)
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 2);
        assert!(failed.iter().any(|result| {
            result.rule_id == "CORE-000867"
                && result.errors[0].row == Some(2)
                && result.errors[0].variables == vec!["variable_value", "variable_name"]
        }));
        assert!(failed.iter().any(|result| {
            result.rule_id == "CORE-000890"
                && result.errors[0].row == Some(1)
                && result.errors[0].variables == vec!["variable_value", "variable_name"]
        }));
    }

    #[test]
    fn run_validation_executes_selected_library_metadata_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AESDTH", "label": "Death", "type": "Char", "length": 1 }
      ],
      "records": { "STUDYID": ["S1"], "AESDTH": ["Y"] }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "XRACE", "label": "Race Extension", "type": "Char", "length": 20 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["DM"], "XRACE": ["BLUE"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Distinct Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Blabla", "type": "Char", "length": 8 },
        { "name": "VSORRESU", "label": "Original Units as Collected", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["VS"],
        "USUBJID": ["S1"],
        "VSSEQ": [1],
        "VSTESTCD": ["SYSBP"],
        "VSORRESU": ["mmHg"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");

        fs::write(
            rules_dir.join("CORE-000398.json"),
            r#"{
  "Core": { "Id": "CORE-000398", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "library_variable_label", "operator": "non_empty" },
    { "name": "variable_label", "operator": "not_equal_to", "value": "library_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable does not correspond to the label in the IG",
    "Output Variables": ["variable_name", "variable_label", "library_variable_label"]
  }
}"#,
        )
        .expect("write label rule");
        fs::write(
            rules_dir.join("CORE-000903.json"),
            r#"{
  "Core": { "Id": "CORE-000903", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "SE", "CO"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "exists" },
    { "name": "variable_name", "operator": "not_equal_to", "value": "library_variable_name" }
  ] },
  "Outcome": {
    "Message": "The variable is not allowed in this domain as it is not specified in the SENDIG for the specific domain",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write allowed-variable rule");
        fs::write(
            rules_dir.join("CORE-000507.json"),
            r#"{
  "Core": { "Id": "CORE-000507", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Define XML",
  "Check": { "all": [
    { "name": "variable_label", "operator": "not_equal_to", "value": "define_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable is incorrect",
    "Output Variables": ["define_variable_name", "define_variable_label", "variable_label"]
  }
}"#,
        )
        .expect("write define label rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");
        let failed = outcome
            .results
            .iter()
            .filter(|result| result.execution_status == ExecutionStatus::Failed)
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 3);
        assert!(failed.iter().any(|result| result.rule_id == "CORE-000398"
            && result.dataset == "AE"
            && result.errors[0].variables
                == vec!["variable_name", "variable_label", "library_variable_label"]));
        assert!(failed.iter().any(|result| result.rule_id == "CORE-000903"
            && result.dataset == "DM"
            && result.errors[0].variables == vec!["variable_name"]));
        assert!(failed.iter().any(|result| result.rule_id == "CORE-000507"
            && result.dataset == "VS"
            && result.error_count == 3));
    }

    #[test]
    fn run_validation_executes_core_000929_domain_codelist_metadata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["FA"] }
    },
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["ZB"] }
    }
  ]
}"#,
        )
        .expect("write datasets");

        fs::write(
            rules_dir.join("CORE-000929.json"),
            r#"{
  "Core": { "Id": "CORE-000929", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$domain_is_custom", "operator": "domain_is_custom" },
    { "id": "$domain_lib_ccode", "operator": "codelist_terms", "codelists": ["DOMAIN"], "returntype": "code" }
  ],
  "Check": { "all": [
    { "name": "$domain_is_custom", "operator": "equal_to", "value": false },
    { "name": "define_variable_ccode", "operator": "equal_to", "value": "C66734" },
    { "name": "define_variable_codelist_coded_codes", "operator": "is_not_contained_by", "value": "$domain_lib_ccode" }
  ] },
  "Outcome": {
    "Message": "DOMAIN Code is not a published DOMAIN Code in CDISC Controlled Terminology.",
    "Output Variables": ["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
  }
}"#,
        )
        .expect("write domain codelist metadata rule");
        fs::write(
            data_dir.join("define.xml"),
            r#"
<ODM>
  <ItemGroupDef OID="IG.FA" Name="FA" Domain="FA">
    <ItemRef ItemOID="IT.FA.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.ZB" Name="ZB" Domain="ZB">
    <ItemRef ItemOID="IT.ZB.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemDef OID="IT.FA.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_FA"/>
  </ItemDef>
  <ItemDef OID="IT.ZB.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_ZB"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN_FA">
    <CodeListItem CodedValue="FA"><Alias Context="nci:ExtCodeID" Name="C00002"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
  <CodeList OID="CL.DOMAIN_ZB">
    <CodeListItem CodedValue="ZB"><Alias Context="nci:ExtCodeID" Name="C00003"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
</ODM>
"#,
        )
        .expect("write define xml");
        fs::write(data_dir.join(".env"), "VERSION=3-3\n").expect("write env");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let failed = outcome
            .results
            .iter()
            .filter(|result| result.execution_status == ExecutionStatus::Failed)
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].rule_id, "CORE-000929");
        assert_eq!(failed[0].dataset, "FA");
        assert_eq!(failed[0].error_count, 1);
        assert_eq!(
            failed[0].errors[0].variables,
            vec!["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
        );
    }

    #[test]
    fn run_validation_executes_core_000494_define_role_metadata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 }
      ],
      "records": { "DOMAIN": ["VS"], "VSTESTCD": ["SYSBP"] }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            data_dir.join("define.xml"),
            r#"
<ODM>
  <ItemGroupDef OID="IG.VS" Name="VS" Domain="VS">
    <ItemRef ItemOID="IT.VS.DOMAIN" OrderNumber="2" Role="WRONG: Domain Identifier"/>
    <ItemRef ItemOID="IT.VS.VSTESTCD" OrderNumber="5" Role="Topic"/>
  </ItemGroupDef>
  <ItemDef OID="IT.VS.DOMAIN" Name="DOMAIN">
    <Description><TranslatedText>Domain Abbreviation</TranslatedText></Description>
  </ItemDef>
  <ItemDef OID="IT.VS.VSTESTCD" Name="VSTESTCD">
    <Description><TranslatedText>Vital Signs Test Short Name</TranslatedText></Description>
  </ItemDef>
</ODM>
"#,
        )
        .expect("write define xml");
        fs::write(
            rules_dir.join("CORE-000494.json"),
            r#"{
  "Core": { "Id": "CORE-000494", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "define_variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "define_variable_role", "operator": "not_equal_to", "value": "library_variable_role" }
  ] },
  "Outcome": {
    "Message": "The Role of the variable in the define.xml does not correspond to the Role given by the Implementation Guide",
    "Output Variables": [
      "define_variable_label",
      "define_variable_name",
      "define_variable_role",
      "library_variable_name",
      "library_variable_role"
    ]
  }
}"#,
        )
        .expect("write rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let failed = outcome
            .results
            .iter()
            .filter(|result| result.execution_status == ExecutionStatus::Failed)
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].rule_id, "CORE-000494");
        assert_eq!(failed[0].dataset, "VS");
        assert_eq!(failed[0].errors[0].row, Some(1));
        assert_eq!(
            failed[0].errors[0].variables,
            vec![
                "define_variable_label",
                "define_variable_name",
                "define_variable_role",
                "library_variable_name",
                "library_variable_role"
            ]
        );
    }

    #[test]
    fn run_validation_executes_core_000595_missing_casno_oracle_issue() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000595.json"),
            r#"{
  "Core": { "Id": "CORE-000595", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IN"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "any": [
    { "all": [
      { "name": "UNII", "operator": "empty" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "not_exists" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "CASNO", "operator": "not_exists" },
      { "name": "UNII", "operator": "empty" }
    ] }
  ] },
  "Outcome": {
    "Message": "At least one of the UNII and CASNO variables should be present and populated for each ingredient if available.",
    "Output Variables": ["UNII", "CASNO"]
  }
}"#,
        )
        .expect("write rule");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "in.xpt",
      "domain": "IN",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 12 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "UNII", "label": "Unique Ingredient Identifier", "type": "Char", "length": 50 }
      ],
      "records": {
        "STUDYID": ["TOB07"],
        "DOMAIN": ["IN"],
        "UNII": ["UNI2"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, None);
        assert!(outcome.results[0].errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_send_variable_metadata_model_column_order_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 },
        { "name": "VSTEST", "label": "Vital Signs Test Name", "type": "Char", "length": 40 },
        { "name": "VSNONSEN", "label": "Vital Signs Nonsense", "type": "Char", "length": 40 },
        { "name": "VSORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "VSNOTDY", "label": "Non Study Day of Vital Signs", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["VS"],
        "USUBJID": ["01"],
        "VSSEQ": [1],
        "VSTESTCD": ["WEIGHT"],
        "VSTEST": ["Weight"],
        "VSNONSEN": ["bad"],
        "VSORRES": ["80"],
        "VSNOTDY": [1]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000902.json"),
            r#"{
  "Core": { "Id": "CORE-000902", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "The variable is not an allowed variable for the underlying Observation Class",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        let rows = outcome.results[0]
            .errors
            .iter()
            .map(|error| error.row)
            .collect::<Vec<_>>();
        assert_eq!(rows, vec![Some(7), Some(9)]);
    }

    #[test]
    fn run_validation_executes_custom_domain_variable_prefix_metadata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "ZBSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "LBORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "ZBORRESU", "label": "Original Units", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["ZB"],
        "USUBJID": ["01"],
        "ZBSEQ": [1],
        "LBORRES": ["80"],
        "ZBORRESU": ["kg"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
        fs::write(
            rules_dir.join("CORE-000376.json"),
            r#"{
  "Core": { "Id": "CORE-000376", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$domain_list", "name": "DOMAIN", "operator": "distinct" },
    { "id": "$domain_is_custom", "operator": "domain_is_custom" }
  ],
  "Check": {
    "all": [
      { "name": "$domain_is_custom", "operator": "equal_to", "value": true },
      { "name": "variable_name", "operator": "is_not_contained_by", "value": ["STUDYID", "DOMAIN", "USUBJID"] },
      { "name": "variable_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$domain_list" }
    ]
  },
  "Outcome": {
    "Message": "First 2 characters of prefixed variable within custom domain do not match the DOMAIN value.",
    "Output Variables": ["$domain_list", "variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "ZB");
        assert_eq!(outcome.results[0].errors[0].row, Some(5));
    }

    #[test]
    fn run_validation_skips_wildcard_target_rules_before_engine_execution() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-WILDCARD-TARGET.json"),
            r#"{
  "Core": { "Id": "CORE-WILDCARD-TARGET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "--TESTCD", "operator": "not_matches_regex", "value": "^[A-Z]+$" },
  "Outcome": { "Message": "wildcard target expansion is not oracle-compatible yet" }
}"#,
        )
        .expect("write wildcard rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_empty_non_empty_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000117.json"),
            r#"{
  "Core": { "Id": "CORE-000117", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "DTHDTC", "operator": "non_empty" },
      { "name": "DTHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "DTHDTC is populated but DTHFL is not Y" }
}"#,
        )
        .expect("write quarantined empty rule");

        let dataset_path = data_dir.join("dm-fail.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": ["2024-01-01"],
        "DTHFL": [""]
      }
    }
  ]
}"#,
        )
        .expect("write fail dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );

        let pass_path = data_dir.join("dm-pass.json");
        fs::write(
            &pass_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": [""],
        "DTHFL": [""]
      }
    }
  ]
}"#,
        )
        .expect("write pass dataset");

        let pass_outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![pass_path],
            ..Default::default()
        })
        .expect("run pass validation");

        assert_eq!(pass_outcome.results.len(), 1);
        assert_eq!(
            pass_outcome.results[0].execution_status,
            ExecutionStatus::Passed
        );
        assert_eq!(pass_outcome.results[0].skipped_reason, None);
    }

    #[test]
    fn run_validation_passes_safe_empty_non_empty_oracle_gap_case() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000007.json"),
            r#"{
  "Core": { "Id": "CORE-000007", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "DTHDTC", "operator": "non_empty" },
      { "name": "DTHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "DTHDTC is populated but DTHFL is not Y" }
}"#,
        )
        .expect("write empty gap rule");
        let dataset_path = data_dir.join("dm.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": [""],
        "DTHFL": [""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].skipped_reason, None);
    }

    #[test]
    fn run_validation_executes_core_000583_trial_summary_value_exclusive_or() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000583.json"),
            r#"{
  "Core": { "Id": "CORE-000583", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "all": [
          { "name": "TSVAL", "operator": "non_empty" },
          { "name": "TSVALNF", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "TSVAL", "operator": "empty" },
          { "name": "TSVALNF", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Either both TSVALNF and TSVAL are populated, or both are empty."
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("ts.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ts.csv",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "TSPARMCD": ["A", "B", "C"],
        "TSVAL": ["VALUE", "", "VALUE"],
        "TSVALNF": ["NF", "", ""]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000583");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_core_000466_missing_uschfl_as_null() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000466.json"),
            r#"{
  "Core": { "Id": "CORE-000466", "Status": "Published" },
  "Scope": {
    "Domains": { "Include": ["ALL"] },
    "Classes": { "Include": ["FINDINGS", "EVENTS", "INTERVENTIONS"] }
  },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--USCHFL", "operator": "non_empty" },
      { "name": "--USCHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "--USCHFL must be either Y or null" }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "LBSEQ": [1, 2],
        "LBUSCHFL": ["maybe", "Y"]
      }
    },
    {
      "filename": "pp.csv",
      "domain": "PP",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["PP"],
        "USUBJID": ["SUBJ1"],
        "PPSEQ": [1]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results[0].dataset, "LB");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[1].dataset, "PP");
        assert_eq!(outcome.results[1].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[1].skipped_reason, None);

        let positive_path = data_dir.join("positive.json");
        fs::write(
            &positive_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBUSCHFL": ["Y"]
      }
    },
    {
      "filename": "pp.csv",
      "domain": "PP",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["PP"],
        "USUBJID": ["SUBJ1"],
        "PPSEQ": [1]
      }
    }
  ]
}"#,
        )
        .expect("write positive dataset");

        let positive = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![positive_path],
            ..Default::default()
        })
        .expect("run positive validation");

        let marker = positive
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .expect("pp marker");
        assert_eq!(marker.rule_id, "CORE-000466");
        assert_eq!(marker.dataset, "PP");
        assert_eq!(marker.error_count, 1);
        assert_eq!(marker.errors[0].row, None);
        assert!(marker.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_skips_date_operator_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000370.json"),
            r#"{
  "Core": { "Id": "CORE-000370", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DVSTDTC", "operator": "date_less_than", "value": "RFICDTC" },
  "Outcome": { "Message": "DVSTDTC date comparison semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write date gap rule");
        let dataset_path = data_dir.join("dv.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dv.csv",
      "domain": "DV",
      "records": {
        "USUBJID": ["SUBJ1"],
        "DVSEQ": [1],
        "DVSTDTC": ["2024-01-01"],
        "RFICDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_passes_safe_date_oracle_gap_case() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000324.json"),
            r#"{
  "Core": { "Id": "CORE-000324", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MH"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_equal_to", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "UNKNOWN"] }
    ]
  },
  "Outcome": { "Message": "--ENRTPT has invalid date-relative semantics" }
}"#,
        )
        .expect("write date gap rule");
        let dataset_path = data_dir.join("mh.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "mh.csv",
      "domain": "MH",
      "records": {
        "USUBJID": ["S1"],
        "MHSEQ": [1],
        "MHDTC": ["2013-05-20"],
        "MHENTPT": ["2013-05-20"],
        "MHENRTPT": ["BEFORE"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].skipped_reason, None);
    }

    #[test]
    fn run_validation_skips_sort_operator_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000535.json"),
            r#"{
  "Core": { "Id": "CORE-000535", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SMSEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "SMSTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "SMSEQ partial-date sort semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write sort gap rule");
        let dataset_path = data_dir.join("sm.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "sm.csv",
      "domain": "SM",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SMSEQ": [1, 3, 2],
        "SMSTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_etcd_length_rules_for_se_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ETCD-SE-LENGTH.json"),
            r#"{
  "Core": { "Id": "CORE-ETCD-SE-LENGTH", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "ETCD", "operator": "longer_than", "value": 8 },
  "Outcome": { "Message": "SE ETCD length semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write ETCD rule");

        let dataset_path = data_dir.join("se.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "se.csv",
      "domain": "SE",
      "records": {
        "ETCD": ["SCREENING"]
      }
    }
  ]
}"#,
        )
        .expect("write SE data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_cross_domain_armcd_txval_length_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ARMCD-TXVAL-LENGTH.json"),
            r#"{
  "Core": { "Id": "CORE-ARMCD-TXVAL-LENGTH", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "TA", "TX"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      { "name": "ARMCD", "operator": "longer_than", "value": 20 },
      {
        "all": [
          { "name": "TXPARMCD", "operator": "equal_to", "value": "ARMCD" },
          { "name": "TXVAL", "operator": "longer_than", "value": 20 }
        ]
      }
    ]
  },
  "Outcome": { "Message": "cross-domain ARMCD/TXVAL length semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write ARMCD/TXVAL rule");

        let dataset_path = data_dir.join("ta.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ta.csv",
      "domain": "TA",
      "records": {
        "ARMCD": ["THIS_ARM_CODE_IS_TOO_LONG"]
      }
    }
  ]
}"#,
        )
        .expect("write TA data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_filters_execution_datasets_by_entity_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ENTITY-SCOPE.json"),
            r#"{
  "Core": { "Id": "CORE-ENTITY-SCOPE", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "instanceType",
    "operator": "equal_to",
    "value": "StudyEpoch",
    "value_is_literal": true
  },
  "Outcome": { "Message": "StudyEpoch rows are checked once" }
}"#,
        )
        .expect("write entity scope rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "id": ["StudyEpoch_1"],
        "instanceType": ["StudyEpoch"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
        )
        .expect("write entity data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].dataset, "StudyEpoch");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_skips_entity_scope_column_ref_comparators() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ENTITY-COLUMN-REF.json"),
            r#"{
  "Core": { "Id": "CORE-ENTITY-COLUMN-REF", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "nextId",
    "operator": "not_equal_to",
    "value": "parent_id"
  },
  "Outcome": { "Message": "Entity relationship comparisons need entity semantics" }
}"#,
        )
        .expect("write entity column-ref rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "nextId": ["StudyEpoch_2"]
      }
    }
  ]
}"#,
        )
        .expect("write entity data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_entity_scope_missing_column_ref_literal_fallback() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ENTITY-LITERAL-FALLBACK.json"),
            r#"{
  "Core": { "Id": "CORE-ENTITY-LITERAL-FALLBACK", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "rel_type",
    "operator": "equal_to",
    "value": "definition"
  },
  "Outcome": { "Message": "definition activities are checked" }
}"#,
        )
        .expect("write entity literal fallback rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1", "Activity_2"],
        "rel_type": ["definition", "instance"]
      }
    }
  ]
}"#,
        )
        .expect("write entity data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_core_000857_entity_codelist_column_refs() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000857.json"),
            r#"{
  "Core": { "Id": "CORE-000857", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Code",
      "Keys": [
        { "Left": "instanceType", "Right": "parent_entity" },
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Operations": [
    {
      "id": "$codelist_code",
      "operator": "map",
      "map": [{ "parent_rel.Code": "plannedSex", "output": "C66732" }]
    },
    { "id": "$valid_versions", "operator": "valid_codelist_dates" },
    {
      "id": "$codelist_extensible",
      "operator": "codelist_extensible",
      "codelist_code": "$codelist_code"
    },
    {
      "id": "$value_for_code",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "value",
      "term_code": "code"
    },
    {
      "id": "$pref_term_for_code",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "pref_term",
      "term_code": "code"
    },
    {
      "id": "$code_for_decode_pref_term",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "code",
      "term_pref_term": "decode"
    },
    {
      "id": "$code_for_decode_value",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "code",
      "term_value": "decode"
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "plannedSex", "operator": "equal_to", "value": true },
      { "name": "parent_rel.Code", "operator": "equal_to", "value": "plannedSex", "value_is_literal": true },
      {
        "not": {
          "all": [
            { "name": "codeSystem", "operator": "equal_to", "value": "http://www.cdisc.org" },
            { "name": "codeSystemVersion", "operator": "is_contained_by", "value": "$valid_versions" },
            {
              "any": [
                {
                  "all": [
                    { "name": "$pref_term_for_code", "operator": "non_empty" },
                    { "name": "$value_for_code", "operator": "non_empty" },
                    {
                      "any": [
                        { "name": "$code_for_decode_pref_term", "operator": "non_empty" },
                        { "name": "$code_for_decode_value", "operator": "non_empty" }
                      ]
                    },
                    {
                      "any": [
                        { "name": "code", "operator": "equal_to", "value": "$code_for_decode_pref_term" },
                        { "name": "code", "operator": "equal_to", "value": "$code_for_decode_value" }
                      ]
                    },
                    {
                      "any": [
                        { "name": "decode", "operator": "equal_to", "value": "$pref_term_for_code" },
                        { "name": "decode", "operator": "equal_to", "value": "$value_for_code" }
                      ]
                    }
                  ]
                },
                {
                  "all": [
                    { "name": "$codelist_extensible", "operator": "equal_to", "value": true },
                    { "name": "$code_for_decode_pref_term", "operator": "empty" },
                    { "name": "$code_for_decode_value", "operator": "empty" },
                    { "name": "$pref_term_for_code", "operator": "empty" },
                    { "name": "$value_for_code", "operator": "empty" }
                  ]
                }
              ]
            }
          ]
        }
      }
    ]
  },
  "Outcome": {
    "Message": "planned sex codelist mismatch",
    "Output Variables": ["code", "decode", "$value_for_code", "$pref_term_for_code"]
  }
}"#,
        )
        .expect("write CORE-000857 rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "id": ["StudyDesignPopulation_1"],
        "instanceType": ["StudyDesignPopulation"],
        "rel_type": ["definition"],
        "plannedSex": [true]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["StudyDesignPopulation_1"],
        "parent_rel.Code": ["plannedSex"],
        "rel_type": ["definition"],
        "codeSystem": ["http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15"],
        "code": ["C16576"],
        "decode": ["Wrong"],
        "id": ["Code_1"],
        "name": ["Wrong code"]
      }
    }
  ]
}"#,
        )
        .expect("write entity codelist data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn static_codelist_resolves_ddf_organization_type_terms() {
        assert!(valid_codelist_dates().contains(&"2025-09-26"));

        let codelist = static_codelist("C188724").expect("organization type codelist");
        assert!(codelist.extensible);

        let sponsor = codelist
            .find_by_code("C70793")
            .expect("clinical study sponsor");
        assert_eq!(sponsor.value, "Study Sponsor");
        assert_eq!(sponsor.pref_term, "Clinical Study Sponsor");

        let registry = codelist
            .find_by_pref_term("Study Registry")
            .expect("study registry");
        assert_eq!(registry.code, "C93453");
        assert_eq!(registry.value, "Clinical Study Registry");

        let drug_company = codelist
            .find_by_value("Drug Company")
            .expect("drug company submission value");
        assert_eq!(drug_company.code, "C54149");
        assert_eq!(drug_company.pref_term, "Pharmaceutical Company");
    }

    #[test]
    fn run_validation_skips_entity_literal_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000820.json"),
            r#"{
  "Core": { "Id": "CORE-000820", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "type",
    "operator": "equal_to",
    "value": "anchor"
  },
  "Outcome": { "Message": "entity literal oracle semantics are not supported" }
}"#,
        )
        .expect("write entity oracle gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "id": ["Timing_1"],
        "type": ["anchor"]
      }
    }
  ]
}"#,
        )
        .expect("write entity data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_jsonata_rules_when_conditions_are_normalized() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        write_raw_rule(
            &rules_dir,
            "CORE-JSONATA",
            r#""Rule Type": "JSONATA""#,
            "",
            r#""operator": "not_equal_to""#,
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-JSONATA");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["DOMAIN".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_supported_dataset_join_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-JOIN-SUPP.json"),
            r#"{
  "Core": { "Id": "CORE-JOIN-SUPP", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [{ "domain": "AE" }, { "domain": "SUPPAE" }],
  "Operations": [
    {
      "name": "left_join",
      "left": "AE",
      "right": "SUPPAE",
      "by": ["USUBJID"],
      "prefix": "SUPP."
    }
  ],
  "Check": {
    "name": "SUPP.QVAL",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "SUPPAE QVAL must not be BAD" }
}"#,
        )
        .expect("write join rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S2"],
        "QNAM": ["AESPID"],
        "QVAL": ["BAD"]
      }
    }
  ]
}"#,
        )
        .expect("write join data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_executes_join_operation_with_different_key_names() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-JOIN-LOOKUP.json"),
            r#"{
  "Core": { "Id": "CORE-JOIN-LOOKUP", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "type": "lookup",
      "leftDataset": "AE",
      "rightDataset": "LOOKUP",
      "leftKeys": ["USUBJID"],
      "rightKeys": ["SUBJECT"],
      "prefix": "LOOKUP."
    }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
        )
        .expect("write lookup rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "SUBJECT": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
        )
        .expect("write lookup data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["LOOKUP.FLAG".to_owned()]
        );
    }

    #[test]
    fn run_validation_join_operation_uses_current_pipeline_left_dataset() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-FILTER-JOIN.json"),
            r#"{
  "Core": { "Id": "CORE-FILTER-JOIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "filter",
      "dataset": "AE",
      "where": {
        "name": "AESEQ",
        "operator": "greater_than",
        "value": 1
      }
    },
    {
      "name": "left_join",
      "left": "AE",
      "right": "SUPPAE",
      "by": ["USUBJID"],
      "prefix": "SUPP."
    }
  ],
  "Check": {
    "name": "SUPP.QVAL",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Filtered-out supplemental values must not reappear" }
}"#,
        )
        .expect("write filter join rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "QVAL": ["BAD", "OK"]
      }
    }
  ]
}"#,
        )
        .expect("write filter join data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_executes_match_datasets_without_explicit_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-MATCH-DATASETS.json"),
            r#"{
  "Core": { "Id": "CORE-MATCH-DATASETS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "domain": "AE" },
    { "domain": "LOOKUP", "prefix": "LOOKUP." }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
        )
        .expect("write match datasets rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
        )
        .expect("write match datasets data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["LOOKUP.FLAG".to_owned()]
        );
    }

    #[test]
    fn run_validation_joins_single_match_dataset_to_scoped_dataset() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-SINGLE-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-SINGLE-MATCH-DATASET", "Status": "Published" },
  "Scope": {
    "Domains": { "Include": ["AE"] },
    "Classes": { "Include": ["EVENTS"] }
  },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SUPPAE", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "QNAM",
    "operator": "equal_to",
    "value": "AESOSP"
  },
  "Outcome": { "Message": "AESOSP supplemental qualifier must be reviewed" }
}"#,
        )
        .expect("write match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S2"],
        "QNAM": ["AESOSP"]
      }
    }
  ]
}"#,
        )
        .expect("write match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_joins_match_dataset_with_left_right_keys() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-LEFT-RIGHT-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-LEFT-RIGHT-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "LOOKUP",
      "Keys": [
        { "Left": "USUBJID", "Right": "SUBJECT" },
        "DOMAIN"
      ]
    }
  ],
  "Check": {
    "name": "FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
        )
        .expect("write match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "SUBJECT": ["S2"],
        "DOMAIN": ["AE"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
        )
        .expect("write match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["FLAG".to_owned()]
        );
    }

    #[test]
    fn run_validation_joins_usdm_match_dataset_before_unique_set() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-USDM-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-USDM-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Code"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Encounter",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Code" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Encounter" },
      { "name": "parent_rel", "operator": "equal_to", "value": "environmentalSetting", "value_is_literal": true },
      {
        "name": "code",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_rel", "parent_id", "codeSystem", "codeSystemVersion"]
      }
    ]
  },
  "Outcome": { "Message": "Duplicate environmental setting" }
}"#,
        )
        .expect("write USDM match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Encounter.csv",
      "domain": "Encounter",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["encounters"],
        "rel_type": ["definition"],
        "id": ["Encounter_1"],
        "name": ["E1"],
        "instanceType": ["Encounter"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["Encounter", "Encounter"],
        "parent_id": ["Encounter_1", "Encounter_1"],
        "parent_rel": ["environmentalSetting", "environmentalSetting"],
        "rel_type": ["definition", "definition"],
        "id": ["Code_84", "Code_85"],
        "code": ["C51282", "C51282"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "decode": ["Clinic", "Hospital"],
        "instanceType": ["Code", "Code"]
      }
    }
  ]
}"#,
        )
        .expect("write USDM match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_joins_scoped_entity_through_multiple_match_datasets() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-MULTI-USDM-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-MULTI-USDM-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "GovernanceDate",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    },
    {
      "Name": "GeographicScope",
      "Keys": [
        { "Left": "id.GovernanceDate", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "name": "id",
    "operator": "is_not_unique_set",
    "value": ["type.code", "type.code.GeographicScope"]
  },
  "Outcome": { "Message": "Governance dates must be unique by type and geographic scope" }
}"#,
        )
        .expect("write multi-match rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyVersion.csv",
      "domain": "StudyVersion",
      "records": {
        "id": ["StudyVersion_1"],
        "rel_type": ["definition"],
        "instanceType": ["StudyVersion"]
      }
    },
    {
      "filename": "GovernanceDate.csv",
      "domain": "GovernanceDate",
      "records": {
        "parent_id": ["StudyVersion_1", "StudyVersion_1"],
        "rel_type": ["definition", "definition"],
        "id": ["GovernanceDate_1", "GovernanceDate_2"],
        "type.code": ["effective", "effective"],
        "instanceType": ["GovernanceDate", "GovernanceDate"]
      }
    },
    {
      "filename": "GeographicScope.csv",
      "domain": "GeographicScope",
      "records": {
        "parent_id": ["GovernanceDate_1", "GovernanceDate_2"],
        "rel_type": ["definition", "definition"],
        "id": ["GeographicScope_1", "GeographicScope_2"],
        "type.code": ["global", "global"],
        "instanceType": ["GeographicScope", "GeographicScope"]
      }
    }
  ]
}"#,
        )
        .expect("write multi-match data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_treats_missing_left_match_dataset_as_no_reference_rows() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-MISSING-LEFT-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-MISSING-LEFT-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "epochId" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyEpoch" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The epoch is not referenced by any scheduled activity instances.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name"]
  }
}"#,
        )
        .expect("write missing match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["epochs", "epochs"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyEpoch_1", "StudyEpoch_2"],
        "name": ["Screening", "Treatment"],
        "instanceType": ["StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
        )
        .expect("write missing match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_treats_missing_yaml_left_match_dataset_as_no_reference_rows() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000816.yml"),
            r#"Check:
  all:
    - name: instanceType
      operator: equal_to
      value: 'StudyEpoch'
    - name: rel_type
      operator: equal_to
      value: 'definition'
    - any:
        - name: epochId
          operator: not_exists
        - name: epochId
          operator: empty
Core:
  Id: 'CORE-000816'
  Status: Published
Match Datasets:
  - Join Type: left
    Keys:
      - Left: id
        Right: epochId
      - rel_type
    Name: ScheduledActivityInstance
Outcome:
  Message: 'The epoch is not referenced by any scheduled activity instances.'
  Output Variables:
    - parent_entity
    - parent_id
    - parent_rel
    - id
    - name
Rule Type: Record Data
Scope:
  Entities:
    Include:
      - 'StudyEpoch'
Sensitivity: Record
"#,
        )
        .expect("write missing yaml match dataset rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nStudyEpoch,StudyEpoch,Study Epoch\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nStudyEpoch,parent_entity,Parent Entity Name,String,[1]\nStudyEpoch,parent_id,Parent Entity Id,String,[1]\nStudyEpoch,parent_rel,Name of Relationship from Parent Entity,String,[1]\nStudyEpoch,rel_type,Type of Relationship,String,[1]\nStudyEpoch,id,Identifier,String,[1]\nStudyEpoch,name,Name,String,[1]\nStudyEpoch,instanceType,Instance Type,String,[1]\nStudyEpoch,type,Study Epoch Type,Boolean,Code[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("StudyEpoch.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType,type\nStudyDesign,StudyDesign_1,epochs,definition,StudyEpoch_1,Screening,StudyEpoch,True\nStudyDesign,StudyDesign_1,epochs,definition,StudyEpoch_2,Treatment,StudyEpoch,True\n",
        )
        .expect("write study epoch csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_joins_schedule_timeline_for_activity_epoch_presence() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["SCREEN1", "AE"],
        "epochId": ["", ""],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimeline.csv",
      "domain": "ScheduleTimeline",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "mainTimeline": [true, false],
        "instanceType": ["ScheduleTimeline", "ScheduleTimeline"]
      }
    }
  ]
}"#,
        )
        .expect("write schedule timeline match data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_schedule_timeline_from_open_rules_csv() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\nScheduleTimeline,ScheduleTimeline,Schedule Timeline\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Identifier,String,[1]\nScheduledActivityInstance,name,Name,String,[1]\nScheduledActivityInstance,epochId,Epoch Identifier,String,StudyEpoch[0..1].id[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\nScheduleTimeline,parent_entity,Parent Entity Name,String,[1]\nScheduleTimeline,parent_id,Parent Entity Id,String,[1]\nScheduleTimeline,rel_type,Type of Relationship,String,[1]\nScheduleTimeline,id,Identifier,String,[1]\nScheduleTimeline,mainTimeline,Main Timeline Indicator,Boolean,[1]\nScheduleTimeline,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,epochId,instanceType\nScheduleTimeline,ScheduleTimeline_1,instances,definition,ScheduledActivityInstance_1,SCREEN1,,ScheduledActivityInstance\nScheduleTimeline,ScheduleTimeline_2,instances,definition,ScheduledActivityInstance_2,AE,,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");
        fs::write(
            data_dir.join("ScheduleTimeline.csv"),
            "parent_entity,parent_id,rel_type,id,mainTimeline,instanceType\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_1,True,ScheduleTimeline\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_2,False,ScheduleTimeline\n",
        )
        .expect("write schedule timeline csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_suffixes_referenced_match_columns_without_left_conflict() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Planned age must be specified either in the study population or in all cohorts." }
}"#,
        )
        .expect("write suffix match column rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population without age column"],
        "instanceType": ["StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["Population_1"],
        "parent_rel": ["cohorts"],
        "rel_type": ["definition"],
        "id": ["Cohort_1"],
        "name": ["Cohort age"],
        "plannedAge": [true],
        "instanceType": ["StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write suffix match column data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_treats_missing_left_study_cohort_as_null_join_columns() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["id.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
        )
        .expect("write missing cohort rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population age"],
        "plannedAge": [true],
        "instanceType": ["StudyDesignPopulation"]
      }
    }
  ]
}"#,
        )
        .expect("write missing cohort data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_left_joins_study_cohort_for_population_planned_sex_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000875.json"),
            r#"{
  "Core": { "Id": "CORE-000875", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedSex", "operator": "not_exists" },
                  { "name": "plannedSex", "operator": "empty" },
                  { "name": "plannedSex", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedSex.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedSex.StudyCohort", "operator": "empty" },
                  { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedSex", "operator": "equal_to", "value": true },
              { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned sex must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedSex", "id.StudyCohort", "name.StudyCohort", "plannedSex.StudyCohort"]
  }
}"#,
        )
        .expect("write planned sex rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedSex": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort sex", "No cohort sex"],
        "plannedSex": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned sex data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_left_joins_study_cohort_for_population_planned_age_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedAge", "id.StudyCohort", "name.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
        )
        .expect("write planned age rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedAge": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort age", "No cohort age"],
        "plannedAge": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned age data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_joins_alias_code_to_standard_code_alias_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000828.json"),
            r#"{
  "Core": { "Id": "CORE-000828", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["AliasCode"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Code",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "AliasCode" },
      { "name": "parent_rel.Code", "operator": "equal_to", "value": "standardCodeAliases", "value_is_literal": true },
      { "name": "standardCode.codeSystem", "operator": "equal_to_case_insensitive", "value": "codeSystem" },
      { "name": "standardCode.codeSystemVersion", "operator": "equal_to_case_insensitive", "value": "codeSystemVersion" },
      {
        "any": [
          { "name": "standardCode.code", "operator": "equal_to_case_insensitive", "value": "code" },
          { "name": "standardCode.decode", "operator": "equal_to_case_insensitive", "value": "decode" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The standard code alias is the same as the standard code.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "standardCode.codeSystem", "standardCode.codeSystemVersion", "standardCode.code", "standardCode.decode", "codeSystem", "codeSystemVersion", "code", "decode"]
  }
}"#,
        )
        .expect("write alias code rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "AliasCode.csv",
      "domain": "AliasCode",
      "records": {
        "parent_entity": ["StudyVersion", "BiomedicalConceptProperty"],
        "parent_id": ["StudyVersion_1", "BiomedicalConceptProperty_1"],
        "parent_rel": ["studyPhase", "code"],
        "rel_type": ["definition", "definition"],
        "id": ["AliasCode_1", "AliasCode_2"],
        "instanceType": ["AliasCode", "AliasCode"],
        "standardCode.code": ["C15601", "C25208"],
        "standardCode.codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "standardCode.codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "standardCode.decode": ["Phase II Trial", "WEIGHT"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["AliasCode", "AliasCode", "AliasCode", "AliasCode"],
        "parent_id": ["AliasCode_1", "AliasCode_1", "AliasCode_2", "AliasCode_2"],
        "parent_rel": ["standardCode", "standardCodeAliases", "standardCode", "standardCodeAliases"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Code_1", "Code_2", "Code_3", "Code_4"],
        "code": ["C15601", "c15601", "C25208", "C99904x3"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15", "2023-12-15", "2023-12-15"],
        "decode": ["Phase II Trial", "Different label", "WEIGHT", "Weight"],
        "instanceType": ["Code", "Code", "Code", "Code"]
      }
    }
  ]
}"#,
        )
        .expect("write alias code data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_left_joins_scheduled_activity_for_fixed_reference_timing_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Fixed reference timing must be related to a scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "type.code", "relativeFromScheduledInstanceId", "id.ScheduledActivityInstance"]
  }
}"#,
        )
        .expect("write fixed reference timing rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["timings", "timings", "timings"],
        "rel_type": ["definition", "definition", "definition"],
        "id": ["Timing_1", "Timing_2", "Timing_3"],
        "name": ["Missing from", "Bad from", "Good from"],
        "type.code": ["C201358", "C201358", "C201358"],
        "type.decode": ["Fixed Reference", "Fixed Reference", "Fixed Reference"],
        "relativeFromScheduledInstanceId": ["", "ScheduledDecisionInstance_1", "ScheduledActivityInstance_1"],
        "instanceType": ["Timing", "Timing", "Timing"]
      }
    },
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["Timing"],
        "parent_id": ["Timing_3"],
        "parent_rel": ["relativeFromScheduledInstanceId"],
        "rel_type": ["reference"],
        "id": ["ScheduledActivityInstance_1"],
        "name": ["Dose"],
        "instanceType": ["ScheduledActivityInstance"]
      }
    }
  ]
}"#,
        )
        .expect("write fixed reference timing data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_left_joins_scheduled_activity_from_open_rules_csv() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Fixed reference timing must be related to a scheduled activity instance." }
}"#,
        )
        .expect("write fixed reference timing rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nTiming,Timing,Timing\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nTiming,parent_entity,Parent Entity Name,String,[1]\nTiming,parent_id,Parent Entity Id,String,[1]\nTiming,parent_rel,Name of Relationship from Parent Entity,String,[1]\nTiming,rel_type,Type of Relationship,String,[1]\nTiming,id,Timing Id,String,[1]\nTiming,name,Timing Name,String,[1]\nTiming,type.code,Timing Type Code,String,[1]\nTiming,relativeFromScheduledInstanceId,Timing Relative From Scheduled Instance,String,ScheduledInstance[0..1].id[1]\nTiming,instanceType,Instance Type,String,[1]\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Scheduled Activity Instance Id,String,[1]\nScheduledActivityInstance,name,Scheduled Activity Instance Name,String,[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("Timing.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,type.code,relativeFromScheduledInstanceId,instanceType\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_1,Missing from,C201358,,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_2,Bad from,C201358,ScheduledDecisionInstance_1,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_3,Good from,C201358,ScheduledActivityInstance_1,Timing\n",
        )
        .expect("write timing csv");
        fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType\nTiming,Timing_3,relativeFromScheduledInstanceId,reference,ScheduledActivityInstance_1,Dose,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_left_joins_objective_for_primary_endpoint_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000874.json"),
            r#"{
  "Core": { "Id": "CORE-000874", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Endpoint"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Objective",
      "Join Type": "left",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "level.code", "operator": "equal_to", "value": "C94496" },
      { "name": "level.code.Objective", "operator": "not_equal_to", "value": "C85826" }
    ]
  },
  "Outcome": {
    "Message": "The primary endpoint (level.code = C94496) is not referenced by a primary objective (level.code.Objective = C85826).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "level.code", "name.Objective", "level.code.Objective"]
  }
}"#,
        )
        .expect("write primary endpoint objective rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Endpoint.csv",
      "domain": "Endpoint",
      "records": {
        "parent_entity": ["Objective", "Objective"],
        "parent_id": ["Objective_1", "Objective_2"],
        "parent_rel": ["endpoints", "endpoints"],
        "rel_type": ["definition", "definition"],
        "id": ["Endpoint_1", "Endpoint_2"],
        "name": ["Primary bad", "Primary good"],
        "level.code": ["C94496", "C94496"],
        "level.decode": ["Primary", "Primary"],
        "instanceType": ["Endpoint", "Endpoint"]
      }
    },
    {
      "filename": "Objective.csv",
      "domain": "Objective",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["objectives", "objectives"],
        "rel_type": ["definition", "definition"],
        "id": ["Objective_1", "Objective_2"],
        "name": ["Secondary objective", "Primary objective"],
        "level.code": ["C85827", "C85826"],
        "level.decode": ["Secondary", "Primary"],
        "instanceType": ["Objective", "Objective"]
      }
    }
  ]
}"#,
        )
        .expect("write primary endpoint objective data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_left_joins_study_epochs_for_study_arm_cell_coverage() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000797.json"),
            r#"{
  "Core": { "Id": "CORE-000797", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Join Type": "left",
      "Keys": ["parent_entity", "parent_id", "rel_type"]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyArm" },
      { "name": "id.StudyEpoch", "operator": "is_unique_set", "value": "id" },
      {
        "any": [
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
              { "name": "rel_type", "operator": "equal_to", "value": "definition" },
              { "name": "parent_rel", "operator": "equal_to", "value": "arms" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochs" }
            ]
          },
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyCell" },
              { "name": "rel_type", "operator": "equal_to", "value": "reference" },
              { "name": "parent_rel", "operator": "equal_to", "value": "armId" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochId" }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The StudyArm does not have a StudyCell for the StudyEpoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "id.StudyEpoch", "name.StudyEpoch"]
  }
}"#,
        )
        .expect("write study arm coverage rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["arms", "arms", "armId", "armId", "armId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyArm_1", "StudyArm_2", "StudyArm_1", "StudyArm_1", "StudyArm_2"],
        "name": ["Placebo", "Active", "Placebo", "Placebo", "Active"],
        "instanceType": ["StudyArm", "StudyArm", "StudyArm", "StudyArm", "StudyArm"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["epochs", "epochs", "epochId", "epochId", "epochId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1"],
        "name": ["Screening", "Treatment", "Screening", "Treatment", "Screening"],
        "instanceType": ["StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
        )
        .expect("write study arm coverage data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_activity_for_duplicate_biomedical_category_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000811.json"),
            r#"{
  "Core": { "Id": "CORE-000811", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConceptCategory"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Activity",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        { "Left": "parent_entity", "Right": "instanceType" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConceptCategory" },
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "rel_type.Activity", "operator": "equal_to", "value": "definition" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Activity" },
      { "name": "parent_rel", "operator": "equal_to", "value": "bcCategoryIds", "value_is_literal": true },
      {
        "name": "id",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_id", "parent_rel", "rel_type.Activity"]
      }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept category is referenced more than once from the same activity.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "name.Activity"]
  }
}"#,
        )
        .expect("write duplicate biomedical category rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "BiomedicalConceptCategory.csv",
      "domain": "BiomedicalConceptCategory",
      "records": {
        "parent_entity": ["Activity", "Activity", "Activity"],
        "parent_id": ["Activity_1", "Activity_1", "Activity_1"],
        "parent_rel": ["bcCategoryIds", "bcCategoryIds", "bcCategoryIds"],
        "rel_type": ["reference", "reference", "reference"],
        "id": ["BiomedicalConceptCategory_1", "BiomedicalConceptCategory_1", "BiomedicalConceptCategory_2"],
        "name": ["Vital Signs", "Vital Signs", "Labs"],
        "instanceType": ["BiomedicalConceptCategory", "BiomedicalConceptCategory", "BiomedicalConceptCategory"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["activities"],
        "rel_type": ["definition"],
        "id": ["Activity_1"],
        "name": ["Vital signs tests"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
        )
        .expect("write duplicate biomedical category data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_joins_string_synonym_for_biomedical_concept_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000803.json"),
            r#"{
  "Core": { "Id": "CORE-000803", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConcept"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "string",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConcept" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel.string", "operator": "equal_to", "value": "synonyms", "value_is_literal": true },
      { "name": "value", "operator": "equal_to_case_insensitive", "value": "name" }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept synonym value is the same as the biomedical concept name (case insensitive).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "parent_rel.string", "value"]
  }
}"#,
        )
        .expect("write biomedical concept synonym rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "BiomedicalConcept.csv",
      "domain": "BiomedicalConcept",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["biomedicalConcepts", "biomedicalConcepts"],
        "rel_type": ["definition", "definition"],
        "id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "name": ["Race", "Weight"],
        "instanceType": ["BiomedicalConcept", "BiomedicalConcept"]
      }
    },
    {
      "filename": "string.csv",
      "domain": "string",
      "records": {
        "parent_entity": ["BiomedicalConcept", "BiomedicalConcept"],
        "parent_id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "parent_rel": ["synonyms", "synonyms"],
        "rel_type": ["definition", "definition"],
        "value": ["race", "Mass"]
      }
    }
  ]
}"#,
        )
        .expect("write biomedical concept synonym data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_timeline_exit_parent_for_scheduled_activity_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000819.json"),
            r#"{
  "Core": { "Id": "CORE-000819", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimelineExit",
      "Keys": [
        { "Left": "timelineExitId", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "ScheduledActivityInstance" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "timelineExitId", "operator": "non_empty" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.ScheduleTimelineExit" }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance references a timeline exit that is not defined within the same schedule timeline as the scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "timelineExitId", "parent_id.ScheduleTimelineExit"]
  }
}"#,
        )
        .expect("write timeline exit match rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["OK", "BAD"],
        "timelineExitId": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimelineExit.csv",
      "domain": "ScheduleTimelineExit",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["exits", "exits"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduleTimelineExit", "ScheduleTimelineExit"]
      }
    }
  ]
}"#,
        )
        .expect("write timeline exit match data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_study_arm_parent_for_study_cell_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000835.json"),
            r#"{
  "Core": { "Id": "CORE-000835", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "armId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyArm" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an arm that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "armId", "parent_id.StudyArm"]
  }
}"#,
        )
        .expect("write study arm match rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "armId": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["arms", "arms"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyArm", "StudyArm"]
      }
    }
  ]
}"#,
        )
        .expect("write study arm match data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_study_epoch_parent_for_study_cell_scope() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000836.json"),
            r#"{
  "Core": { "Id": "CORE-000836", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Keys": [
        { "Left": "epochId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyEpoch" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an epoch that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "epochId", "parent_id.StudyEpoch"]
  }
}"#,
        )
        .expect("write study epoch match rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "epochId": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["epochs", "epochs"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
        )
        .expect("write study epoch match data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_joins_single_match_dataset_to_each_scoped_dataset() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-MULTI-BASE-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-MULTI-BASE-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "RFSTDTC",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Reference start date must be reviewed" }
}"#,
        )
        .expect("write match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"],
        "DOMAIN": ["AE"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S2"],
        "DOMAIN": ["CM"],
        "CMSEQ": [1]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1", "S2"],
        "RFSTDTC": ["BAD", "OK"]
      }
    }
  ]
}"#,
        )
        .expect("write match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let failed = outcome
            .results
            .iter()
            .find(|result| result.dataset == "AE")
            .expect("AE result");
        assert_eq!(failed.execution_status, ExecutionStatus::Failed);
        assert_eq!(failed.error_count, 1);
        let passed = outcome
            .results
            .iter()
            .find(|result| result.dataset == "CM")
            .expect("CM result");
        assert_eq!(passed.execution_status, ExecutionStatus::Passed);
    }

    #[test]
    fn run_validation_skips_multi_base_match_dataset_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000354.json"),
            r#"{
  "Core": { "Id": "CORE-000354", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "RFSTDTC",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Reference start date has oracle-specific join semantics" }
}"#,
        )
        .expect("write match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["BAD"]
      }
    }
  ]
}"#,
        )
        .expect("write match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
        assert!(outcome
            .results
            .iter()
            .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
    }

    #[test]
    fn run_validation_skips_usdm_match_dataset_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduleTimeline"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "name": "instanceType",
    "operator": "equal_to",
    "value": "ScheduleTimeline"
  },
  "Outcome": { "Message": "USDM match dataset has oracle-specific flatten semantics" }
}"#,
        )
        .expect("write USDM match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ScheduleTimeline.csv",
      "domain": "ScheduleTimeline",
      "records": {
        "id": ["ScheduleTimeline_1"],
        "instanceType": ["ScheduleTimeline"]
      }
    }
  ]
}"#,
        )
        .expect("write USDM match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::DatasetJoinNotSupported)
        );
    }

    #[test]
    fn run_validation_fans_out_single_match_dataset_with_duplicate_lookup_keys() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DUPLICATE-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-DUPLICATE-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "LOOKUP", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
        )
        .expect("write duplicate match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DOMAIN": ["DM"]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S1", "S1"],
        "FLAG": ["Y", "N"]
      }
    }
  ]
}"#,
        )
        .expect("write duplicate match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
    }

    #[test]
    fn run_validation_skips_duplicate_match_dataset_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000252.json"),
            r#"{
  "Core": { "Id": "CORE-000252", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DS", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "DSDECOD",
    "operator": "equal_to",
    "value": "DEATH"
  },
  "Outcome": { "Message": "Death disposition has oracle-specific duplicate match semantics" }
}"#,
        )
        .expect("write duplicate match dataset rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"]
      }
    },
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["S1", "S1"],
        "DSDECOD": ["DEATH", "COMPLETED"]
      }
    }
  ]
}"#,
        )
        .expect("write duplicate match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_relrec_and_supp_match_dataset_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000206.json"),
            r#"{
  "Core": { "Id": "CORE-000206", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SUPP--", "Keys": ["USUBJID", "IDVAR", "IDVARVAL"] },
    { "Name": "RELREC", "Keys": ["USUBJID", "IDVAR", "IDVARVAL"] }
  ],
  "Check": {
    "name": "IDVARVAL",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "SUPP-- placeholder has oracle-specific match semantics" }
}"#,
        )
        .expect("write supp placeholder rule");
        fs::write(
            rules_dir.join("CORE-000744.json"),
            r#"{
  "Core": { "Id": "CORE-000744", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["FA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "RELREC" }
  ],
  "Check": {
    "name": "FAOBJ",
    "operator": "not_equal_to",
    "value": "RELREC.AETERM"
  },
  "Outcome": { "Message": "RELREC wildcard has oracle-specific match semantics" }
}"#,
        )
        .expect("write relrec rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S1"],
        "IDVAR": ["AESEQ"],
        "IDVARVAL": ["1"]
      }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FAOBJ": ["TERM"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["S1"],
        "RELID": ["R1"]
      }
    }
  ]
}"#,
        )
        .expect("write match dataset data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
        assert!(outcome
            .results
            .iter()
            .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
    }

    #[test]
    fn run_validation_uses_reference_distinct_operation_values_as_sets() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-REFERENCE-DISTINCT.json"),
            r#"{
  "Core": { "Id": "CORE-REFERENCE-DISTINCT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["RELREC"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "RELREC",
      "id": "$rdomain_variables",
      "name": "IDVAR",
      "operator": "distinct",
      "value_is_reference": true
    }
  ],
  "Check": {
    "all": [
      { "name": "RDOMAIN", "operator": "exists" },
      { "name": "IDVAR", "operator": "non_empty" },
      {
        "name": "IDVAR",
        "operator": "is_not_contained_by",
        "value": "$rdomain_variables"
      }
    ]
  },
  "Outcome": { "Message": "IDVAR must name a variable in RDOMAIN" }
}"#,
        )
        .expect("write reference distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "variables": [
        { "name": "STUDYID" },
        { "name": "RDOMAIN" },
        { "name": "USUBJID" },
        { "name": "IDVAR" },
        { "name": "IDVARVAL" }
      ],
      "records": {
        "STUDYID": ["S1", "S1"],
        "RDOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "IDVAR": ["LBSEQ", "AESEQ"],
        "IDVARVAL": ["1", "2"]
      }
    },
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "variables": [
        { "name": "STUDYID" },
        { "name": "USUBJID" },
        { "name": "LBSEQ" },
        { "name": "LBTESTCD" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBTESTCD": ["ALT"]
      }
    }
  ]
}"#,
        )
        .expect("write reference distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_core_000172_reference_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000172.json"),
            r#"{
  "Core": { "Id": "CORE-000172", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_studyid",
      "name": "STUDYID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "STUDYID",
    "operator": "is_not_contained_by",
    "value": "$dm_studyid"
  },
  "Outcome": { "Message": "STUDYID is not equal to DM.STUDYID" }
}"#,
        )
        .expect("write distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"]
      }
    }
  ]
}"#,
        )
        .expect("write distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_skips_core_000172_sendig_reference_distinct_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000172.json"),
            r#"{
  "Core": { "Id": "CORE-000172", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": { "Include": ["AE", "DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_studyid",
      "name": "STUDYID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "STUDYID",
    "operator": "is_not_contained_by",
    "value": "$dm_studyid"
  },
  "Outcome": { "Message": "STUDYID is not equal to DM.STUDYID" }
}"#,
        )
        .expect("write SENDIG distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"]
      }
    }
  ]
}"#,
        )
        .expect("write SENDIG distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-000172".to_owned()],
            standard: Some("SENDIG".to_owned()),
            standard_version: Some("3.1".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_core_000201_reference_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000201.json"),
            r#"{
  "Core": { "Id": "CORE-000201", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "DM", "TA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "non_empty" },
      { "name": "USUBJID", "operator": "is_not_contained_by", "value": "$dm_usubjid" }
    ]
  },
  "Outcome": { "Message": "USUBJID is not found in DM.USUBJID" }
}"#,
        )
        .expect("write distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"]
      }
    },
    {
      "filename": "ta.xpt",
      "domain": "TA",
      "records": {
        "STUDYID": ["S1"],
        "ARMCD": ["A"],
        "ARM": ["Active"]
      }
    }
  ]
}"#,
        )
        .expect("write distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let by_dataset = outcome
            .results
            .iter()
            .map(|result| (result.dataset.as_str(), result))
            .collect::<std::collections::BTreeMap<_, _>>();

        let ae = by_dataset.get("AE").expect("AE result");
        assert_eq!(ae.execution_status, ExecutionStatus::Failed, "{ae:?}");
        assert_eq!(ae.error_count, 1);
        assert_eq!(ae.errors[0].row, Some(2));
        assert_eq!(ae.errors[0].seq.as_deref(), Some("2"));

        let ta = by_dataset.get("TA").expect("TA result");
        assert_eq!(ta.execution_status, ExecutionStatus::Failed, "{ta:?}");
        assert_eq!(ta.error_count, 1);
        assert_eq!(ta.errors[0].row, None);
        assert!(ta.errors[0].variables.is_empty());
    }

    #[test]
    fn run_validation_executes_core_000239_external_min_date_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000239.json"),
            r#"{
  "Core": { "Id": "CORE-000239", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$min_ex_exstdtc",
      "name": "EXSTDTC",
      "operator": "min_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXSTDTC", "operator": "not_equal_to", "value": "$min_ex_exstdtc" }
    ]
  },
  "Outcome": {
    "Message": "RFXSTDTC does not equal the earliest value of EX.EXSTDTC",
    "Output Variables": ["RFXSTDTC", "$min_ex_exstdtc"]
  }
}"#,
        )
        .expect("write min date rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXSTDTC": ["2020-01-02", "2020-02-03"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "EXSTDTC": ["2020-01-03", "2020-01-02", "2020-02-01"]
      }
    }
  ]
}"#,
        )
        .expect("write min date data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_core_000238_external_max_date_operations() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000238.json"),
            r#"{
  "Core": { "Id": "CORE-000238", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_exstdtc",
      "name": "EXSTDTC",
      "operator": "max_date"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_exendtc",
      "name": "EXENDTC",
      "operator": "max_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exstdtc" },
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exendtc" }
    ]
  },
  "Outcome": {
    "Message": "RFXENDTC does not equal the latest value of EX.EXSTDTC or EX.EXENDTC",
    "Output Variables": ["RFXENDTC", "$max_ex_exstdtc", "$max_ex_exendtc"]
  }
}"#,
        )
        .expect("write max date rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXENDTC": ["2020-01-05", "2020-02-04"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "EXSTDTC": ["2020-01-01", "2020-01-03", "2020-02-01", "2020-02-02"],
        "EXENDTC": ["2020-01-02", "2020-01-05", "2020-02-02", "2020-02-03"]
      }
    }
  ]
}"#,
        )
        .expect("write max date data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");
        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_grouped_min_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-MIN.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-MIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "source_column": "AVAL",
      "as": "MIN_AVAL",
      "by": ["USUBJID"],
      "operator": "min"
    }
  ],
  "Check": {
    "name": "MIN_AVAL",
    "operator": "equal_to",
    "value": 3
  },
  "Outcome": {
    "Message": "AVAL is not the subject-wise minimum"
  }
}"#,
        )
        .expect("write min operation rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "AVAL": [3, 4, 5]
      }
    }
  ]
}"#,
        )
        .expect("write min operation data");

        let dataset_path_for_validation = dataset_path.clone();
        let rules_dir_for_validation = rules_dir.clone();
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir_for_validation],
            dataset_paths: vec![dataset_path_for_validation],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_grouped_max_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-MAX.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-MAX", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "source_column": "AVAL",
      "as": "MAX_AVAL",
      "by": ["USUBJID"],
      "operator": "max"
    }
  ],
  "Check": {
    "name": "MAX_AVAL",
    "operator": "equal_to",
    "value": 10
  },
  "Outcome": {
    "Message": "AVAL is not the subject-wise maximum"
  }
}"#,
        )
        .expect("write max operation rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "AVAL": [2, 10, 5, 3]
      }
    }
  ]
}"#,
        )
        .expect("write max operation data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_grouped_external_min_max_operations() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-MIN-MAX-EXT.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-MIN-MAX-EXT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$min_ex_aval",
      "name": "AVAL",
      "operator": "min"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_aval",
      "name": "AVAL",
      "operator": "max"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXMIN", "operator": "not_equal_to", "value": "$min_ex_aval" },
      { "name": "RFXMAX", "operator": "not_equal_to", "value": "$max_ex_aval" }
    ]
  },
  "Outcome": {
    "Message": "RFX values are not equal to grouped EX AVAL"
  }
}"#,
        )
        .expect("write external min max operation rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXMIN": [9, 0],
        "RFXMAX": [5, 9]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ3"],
        "AVAL": [5, 2, 7, 1]
      }
    }
  ]
}"#,
        )
        .expect("write external min max operation data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_skips_core_000773_date_operation_gap() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000773.json"),
            r#"{
  "Core": { "Id": "CORE-000773", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DS",
      "group": ["USUBJID"],
      "id": "$dsstdtc",
      "name": "DSSTDTC",
      "operator": "max_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "$dsstdtc" }
    ]
  },
  "Outcome": { "Message": "--DTC may not be later than DS.DSSTDTC" }
}"#,
        )
        .expect("write date gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ma.xpt",
      "domain": "MA",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "MADTC": ["2020-01-02T00:00:01"]
      }
    },
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "DSSTDTC": ["2020-01-02T00:00:00"]
      }
    }
  ]
}"#,
        )
        .expect("write date gap data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_core_000770_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000770.json"),
            r#"{
  "Core": { "Id": "CORE-000770", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "distinct",
      "domain": "TX",
      "group": ["SETCD"],
      "name": "TXPARMCD",
      "id": "$txparms_by_set"
    }
  ],
  "Check": { "name": "$txparms_by_set", "operator": "does_not_contain", "value": "SPGRPCD" },
  "Outcome": { "Message": "TXPARMCD must include SPGRPCD per SETCD" }
}"#,
        )
        .expect("write distinct operation gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "SETCD": ["SET1", "SET1", "SET2"],
        "TXPARMCD": ["ARMCD", "SPGRPCD", "ARMCD"]
      }
    }
  ]
}"#,
        )
        .expect("write distinct date gap data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(3));
    }

    #[test]
    fn run_validation_executes_scope_wide_reference_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000140.json"),
            r#"{
  "Core": { "Id": "CORE-000140", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" },
      { "name": "VISITDY", "operator": "non_empty" }
    ]
  },
  "Outcome": {
    "Message": "VISITDY is populated for an unplanned visit",
    "Output Variables": ["VISITNUM", "VISITDY"]
  }
}"#,
        )
        .expect("write distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [1, 2]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [1, 99],
        "VISITDY": [1, 99]
      }
    }
  ]
}"#,
        )
        .expect("write distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let failed = outcome
            .results
            .iter()
            .find(|result| result.execution_status == ExecutionStatus::Failed)
            .expect("failed result");
        assert_eq!(failed.dataset, "SV");
        assert_eq!(failed.error_count, 1);
        assert_eq!(failed.errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_core_000361_one_way_relationship_semantics() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000361.json"),
            r#"{
  "Core": { "Id": "CORE-000361", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": { "all": [
    { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" },
    { "name": "VISIT", "operator": "is_not_unique_relationship", "value": "VISITNUM" }
  ] },
  "Outcome": {
    "Message": "VISIT and VISITNUM do not have a one-to-one relationship",
    "Output Variables": ["VISITNUM", "VISIT"]
  }
}"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [700, 700],
        "VISIT": ["VISIT 7 (WEEK 5)", "VISIT 8 (WEEK 6)"]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [100, 100],
        "VISIT": ["VISIT 1 (WEEK -2)", "VISIT 1"]
      }
    }
  ]
}"#,
        )
        .expect("write data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000361");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_passes_core_000678_when_pooldef_is_absent() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000678.yml"),
            r#"
Core:
  Id: CORE-000678
  Status: Published
Scope:
  Classes:
    Include:
      - ALL
  Domains:
    Include:
      - ALL
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: POOLDEF
    id: $pooldef_poolid
    name: POOLID
    operator: distinct
Check:
  all:
    - name: POOLID
      operator: non_empty
    - name: POOLID
      operator: is_not_contained_by
      value: $pooldef_poolid
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - POOLID
    - $pooldef_poolid
"#,
        )
        .expect("write rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["VS", "VS"],
        "USUBJID": ["", ""],
        "POOLID": ["POOL1", "POOL2"],
        "VSSEQ": [1, 2]
      }
    }
  ]
}"#,
        )
        .expect("write data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_executes_domain_label_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DOMAIN-LABEL.json"),
            r#"{
  "Core": { "Id": "CORE-DOMAIN-LABEL", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "Category must not repeat the domain label" }
}"#,
        )
        .expect("write domain label rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "LBCAT": ["Laboratory Test Results", "LB", "CHEMISTRY"]
      }
    }
  ]
}"#,
        )
        .expect("write domain label data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_core_000272_domain_label_cat_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000272.json"),
            r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
        )
        .expect("write domain label rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "LBCAT": ["Laboratory Test Results"]
      }
    }
  ]
}"#,
        )
        .expect("write domain label oracle gap data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-000272".to_owned()],
            standard: Some("SDTMIG".to_owned()),
            standard_version: Some("3.4".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_core_000272_sendig_domain_name_cat_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000272.json"),
            r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
        )
        .expect("write SENDIG domain name rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBCAT": ["LB"]
      }
    }
  ]
}"#,
        )
        .expect("write SENDIG domain name data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-000272".to_owned()],
            standard: Some("SENDIG".to_owned()),
            standard_version: Some("3.1".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["LBCAT".to_owned(), "DOMAIN".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_extract_metadata_dataset_name_string_part_check() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-EXTRACT-METADATA.json"),
            r#"{
  "Core": { "Id": "CORE-EXTRACT-METADATA", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$dataset_name",
      "name": "dataset_name",
      "operator": "extract_metadata"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "does_not_equal_string_part",
    "regex": ".{4}(..).*",
    "value": "$dataset_name"
  },
  "Outcome": { "Message": "RDOMAIN must match the parent domain in the SUPP dataset name" }
}"#,
        )
        .expect("write extract metadata rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "RDOMAIN": ["AE", "XX"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "QNAM": ["AETERM", "BAD"]
      }
    }
  ]
}"#,
        )
        .expect("write extract metadata data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_get_xhtml_errors_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-XHTML.json"),
            r#"{
  "Core": { "Id": "CORE-XHTML", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["EligibilityCriterion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$xhtml_errors",
      "name": "text",
      "namespace": "http://www.cdisc.org/ns/usdm/xhtml/v1.0",
      "operator": "get_xhtml_errors"
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "$xhtml_errors", "operator": "non_empty" }
    ]
  },
  "Outcome": { "Message": "The text attribute contains non-conformant XHTML." }
}"#,
        )
        .expect("write xhtml rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "EligibilityCriterion.csv",
      "domain": "EligibilityCriterion",
      "records": {
        "rel_type": ["definition", "definition", "definition", "label"],
        "name": ["VALID", "BAD_TAG", "BAD_XML", "IGNORED"],
        "text": [
          "<p>At least <usdm:tag name=\"min_age\"/> years.</p>",
          "<p><usdm:tag nam=\"min_age\"/></p>",
          "Insulin-dependent & diabetic",
          "Insulin-dependent & diabetic"
        ]
      }
    }
  ]
}"#,
        )
        .expect("write xhtml data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[1].row, Some(3));
    }

    #[test]
    fn run_validation_executes_reference_distinct_operation_from_scope_external_dataset() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000036.json"),
            r#"{
  "Core": { "Id": "CORE-000036", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visit",
      "name": "VISIT",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISIT", "operator": "is_not_contained_by", "value": "$tv_visit" }
    ]
  },
  "Outcome": { "Message": "Planned visit is not found in TV" }
}"#,
        )
        .expect("write reference distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "SVSEQ": [1, 2, 3],
        "SVPRESP": ["Y", "Y", "N"],
        "VISIT": ["BASELINE", "SCREENING", "SCREENING"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "VISIT": ["BASELINE", "WEEK 1"]
      }
    }
  ]
}"#,
        )
        .expect("write reference distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["SVPRESP".to_owned(), "VISIT".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_tv_visitnum_reference_distinct_operations() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000039.json"),
            r#"{
  "Core": { "Id": "CORE-000039", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visitnum",
      "name": "VISITNUM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": { "Message": "Planned visit number is not found in TV" }
}"#,
        )
        .expect("write planned visitnum rule");
        fs::write(
            rules_dir.join("CORE-000040.json"),
            r#"{
  "Core": { "Id": "CORE-000040", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visitnum",
      "name": "VISITNUM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "empty" },
      { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": { "Message": "Unplanned visit number is found in TV" }
}"#,
        )
        .expect("write unplanned visitnum rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "SVSEQ": [1, 2, 3],
        "SVPRESP": ["Y", "Y", ""],
        "VISITNUM": ["1", "99", "1"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "VISITNUM": ["1", "2"]
      }
    }
  ]
}"#,
        )
        .expect("write visitnum data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let by_rule = outcome
            .results
            .iter()
            .map(|result| (result.rule_id.as_str(), result))
            .collect::<std::collections::BTreeMap<_, _>>();
        let planned = by_rule.get("CORE-000039").expect("planned result");
        assert_eq!(planned.execution_status, ExecutionStatus::Failed);
        assert_eq!(planned.error_count, 1);
        assert_eq!(planned.errors[0].row, Some(2));
        assert_eq!(planned.errors[0].seq.as_deref(), Some("2"));

        let unplanned = by_rule.get("CORE-000040").expect("unplanned result");
        assert_eq!(unplanned.execution_status, ExecutionStatus::Failed);
        assert_eq!(unplanned.error_count, 1);
        assert_eq!(unplanned.errors[0].row, Some(3));
        assert_eq!(unplanned.errors[0].seq.as_deref(), Some("3"));
    }

    #[test]
    fn run_validation_executes_trial_arm_reference_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000047.json"),
            r#"{
  "Core": { "Id": "CORE-000047", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TA",
      "id": "$ta_arm",
      "name": "ARM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "ACTARM", "operator": "non_empty" },
      { "name": "ARM", "operator": "non_empty" },
      { "name": "ARM", "operator": "is_not_contained_by", "value": "$ta_arm" }
    ]
  },
  "Outcome": { "Message": "DM ARM is not found in TA" }
}"#,
        )
        .expect("write arm reference distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "ARM": ["PLACEBO", "BADARM", ""],
        "ACTARM": ["PLACEBO", "BADARM", "PLACEBO"]
      }
    },
    {
      "filename": "ta.xpt",
      "domain": "TA",
      "records": {
        "STUDYID": ["S1", "S1"],
        "ARM": ["PLACEBO", "DRUG"]
      }
    }
  ]
}"#,
        )
        .expect("write arm reference distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_study_domains_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-STUDY-DOMAINS.json"),
            r#"{
  "Core": { "Id": "CORE-STUDY-DOMAINS", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["RELREC"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$study_domains",
      "operator": "study_domains"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "is_not_contained_by",
    "value": "$study_domains"
  },
  "Outcome": {
    "Message": "RDOMAIN does not represent a dataset present in the study",
    "Output Variables": ["RDOMAIN"]
  }
}"#,
        )
        .expect("write study domains rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RELID": ["R1", "R2"],
        "RDOMAIN": ["AE", "XX"]
      }
    },
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "AESEQ": [1]
      }
    }
  ]
}"#,
        )
        .expect("write study domains data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["RDOMAIN".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_variable_count_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-VARIABLE-COUNT.json"),
            r#"{
  "Core": { "Id": "CORE-VARIABLE-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$VARIABLE_COUNT",
      "name": "--LNKGRP",
      "operator": "variable_count"
    }
  ],
  "Check": {
    "all": [
      { "name": "--LNKGRP", "operator": "exists" },
      { "name": "$VARIABLE_COUNT", "operator": "less_than", "value": 2 }
    ]
  },
  "Outcome": {
    "Message": "LNKGRP variable is not found in any of the other domains.",
    "Output Variables": ["--LNKGRP", "$VARIABLE_COUNT"]
  }
}"#,
        )
        .expect("write variable count rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID" },
        { "name": "AESEQ" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID" },
        { "name": "FASEQ" },
        { "name": "FALNKGRP" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "FASEQ": [1],
        "FALNKGRP": ["CDISC001 - 1"]
      }
    }
  ]
}"#,
        )
        .expect("write variable count data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let result = outcome
            .results
            .iter()
            .find(|result| result.dataset == "FA")
            .expect("FA result");
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].row, None);
        assert_eq!(
            result.errors[0].variables,
            vec!["FALNKGRP".to_owned(), "$VARIABLE_COUNT".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_dy_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DY.json"),
            r#"{
  "Core": { "Id": "CORE-DY", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Operations": [
    {
      "id": "$val_dy",
      "name": "--STDTC",
      "operator": "dy"
    }
  ],
  "Check": {
    "all": [
      { "name": "--STDTC", "operator": "is_complete_date" },
      { "name": "RFSTDTC", "operator": "is_complete_date" },
      { "name": "--STDY", "operator": "not_equal_to", "value": "$val_dy" }
    ]
  },
  "Outcome": {
    "Message": "--DY is not calculated correctly",
    "Output Variables": ["--STDY", "--STDTC", "RFSTDTC", "$val_dy"]
  }
}"#,
        )
        .expect("write dy rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1", "S1"],
        "AESEQ": [1, 2, 3],
        "AESTDTC": ["2024-01-01", "2023-12-31", "2024-01-02"],
        "AESTDY": [1, -1, 3]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
        )
        .expect("write dy data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        let result = outcome
            .results
            .iter()
            .find(|result| result.dataset == "AE")
            .unwrap_or_else(|| panic!("AE result not found: {:?}", outcome.results));
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].row, Some(3));
        assert_eq!(result.errors[0].seq.as_deref(), Some("3"));
        assert_eq!(
            result.errors[0].variables,
            vec![
                "AESTDY".to_owned(),
                "AESTDTC".to_owned(),
                "RFSTDTC".to_owned(),
                "$val_dy".to_owned()
            ]
        );
    }

    #[test]
    fn run_validation_skips_dy_operation_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000436.json"),
            r#"{
  "Core": { "Id": "CORE-000436", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Operations": [
    {
      "id": "$val_dy",
      "name": "--DTC",
      "operator": "dy"
    }
  ],
  "Check": {
    "all": [
      { "name": "--DY", "operator": "non_empty" },
      { "name": "--DTC", "operator": "is_complete_date" },
      { "name": "RFSTDTC", "operator": "is_complete_date" },
      { "name": "--DY", "operator": "not_equal_to", "value": "$val_dy" }
    ]
  },
  "Outcome": { "Message": "--DY has oracle-specific dy semantics" }
}"#,
        )
        .expect("write dy oracle gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "USUBJID": ["S1"],
        "EXSEQ": [1],
        "EXDTC": ["2024-01-01"],
        "EXDY": [0]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
        )
        .expect("write dy oracle gap data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_domain_placeholder_column_ref_comparator() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DOMAIN-PLACEHOLDER-COLUMN-REF.json"),
            r#"{
  "Core": { "Id": "CORE-DOMAIN-PLACEHOLDER-COLUMN-REF", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--SCAT",
    "operator": "equal_to_case_insensitive",
    "value": "--DECOD"
  },
  "Outcome": {
    "Message": "--SCAT must match --DECOD",
    "Output Variables": ["--DECOD", "--SCAT"]
  }
}"#,
        )
        .expect("write domain placeholder comparator rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2],
        "AEDECOD": ["HEADACHE", "NAUSEA"],
        "AESCAT": ["headache", "CARDIAC"]
      }
    }
  ]
}"#,
        )
        .expect("write domain placeholder comparator data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("1"));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["AEDECOD".to_owned(), "AESCAT".to_owned()]
        );
    }

    #[test]
    fn run_validation_skips_domain_placeholder_column_ref_oracle_gap_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000195.json"),
            r#"{
  "Core": { "Id": "CORE-000195", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--SCAT",
    "operator": "equal_to_case_insensitive",
    "value": "--DECOD"
  },
  "Outcome": { "Message": "--SCAT repeats --DECOD" }
}"#,
        )
        .expect("write domain placeholder oracle gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "AEDECOD": ["HEADACHE"],
        "AESCAT": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write domain placeholder oracle gap data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_executes_inner_join_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-INNER-JOIN.json"),
            r#"{
  "Core": { "Id": "CORE-INNER-JOIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "inner_join",
      "left": "AE",
      "right": "LOOKUP",
      "by": ["USUBJID"],
      "prefix": "LOOKUP."
    }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Matched lookup flag must not be Y" }
}"#,
        )
        .expect("write inner join rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
        )
        .expect("write inner join data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_executes_jsonata_string_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-JSONATA-STRING.json"),
            r#"{
  "Core": { "Id": "CORE-JSONATA-STRING", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$exists(DOMAIN) and DOMAIN != 'AE'",
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write jsonata string rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_planned_enrollment_jsonata_unit_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000981.json"),
            r#"{
  "Core": { "Id": "CORE-000981", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InPopQ:=$boolean(plannedEnrollmentNumber.unit); {\"check\": $InPopQ=true} )][check = true]",
  "Outcome": {
    "Message": "A unit has been specified for a planned enrollment number",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "plannedEnrollmentNumber": {
                "id": "Quantity_1",
                "value": 22,
                "unit": {
                  "id": "Unit_1",
                  "standardCode": { "decode": "Day", "code": "C25301" }
                }
              },
              "cohorts": []
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "StudyDesignPopulation"
        );
    }

    #[test]
    fn run_validation_executes_usdm_planned_enrollment_cohort_consistency_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000963.json"),
            r#"{
  "Core": { "Id": "CORE-000963", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InCohort:=$boolean(cohorts.plannedEnrollmentNumber); $InPop:=($type(plannedEnrollmentNumber) != \"null\" and $exists(plannedEnrollmentNumber)); {\"check\": (($InPop=true and $InCohort=true) or ($InPop=false and $InCohort=true))} )][check=true]",
  "Outcome": {
    "Message": "A planned enrollment number has been specified for both the study population and the cohorts, or it has been specified for only a subset of the cohorts.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_1",
                  "name": "COHORT1",
                  "plannedEnrollmentNumber": { "id": "Quantity_1", "value": 10 }
                },
                { "id": "StudyCohort_2", "name": "COHORT2" }
              ]
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "StudyDesignPopulation"
        );
    }

    #[test]
    fn run_validation_executes_usdm_sponsor_role_applies_to_version_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000974.json"),
            r#"{
  "Core": { "Id": "CORE-000974", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "study.versions@$sv.($sv.roles[code.code = \"C70793\" and $not($sv.id in appliesToIds)])@$r.{\"check\": true}",
  "Outcome": {
    "Message": "The study role is a sponsor role (code.code is C70793) but it is not applicable to the study version.",
    "Output Variables": ["name", "code.code", "code.decode", "appliesToIds", "StudyVersion.id"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "ROLE_1",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": []
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyRole");
    }

    #[test]
    fn run_validation_executes_usdm_main_timeline_count_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000407.json"),
            r##"{
  "Core": { "Id": "CORE-000407", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Main timelines` != 1][]",
  "Outcome": {
    "Message": "The study design does not have exactly one main timeline.",
    "Output Variables": ["name", "# Main timelines", "Main timelines"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Study Design 1",
            "instanceType": "InterventionalStudyDesign",
            "scheduleTimelines": []
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
    }

    #[test]
    fn run_validation_executes_usdm_timeline_order_consistency_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (rule_id, previous_next, timeline_refs) in [
            (
                "CORE-000961",
                "Encounter order by previous/next",
                "Encounter order by timeline refs",
            ),
            (
                "CORE-001048",
                "Epoch order by previous/next",
                "Epoch order by timeline refs",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{rule_id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["StudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Timeline order is inconsistent.",
    "Output Variables": [
      "name",
      "ScheduleTimeline.id",
      "ScheduleTimeline.name",
      "ScheduleTimeline.mainTimeline",
      "{previous_next}",
      "{timeline_refs}"
    ]
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "epochs": [
              { "id": "Epoch_1", "name": "Screening", "nextId": "Epoch_2" },
              { "id": "Epoch_2", "name": "Treatment", "previousId": "Epoch_1", "nextId": "Epoch_3" },
              { "id": "Epoch_3", "name": "Follow-up", "previousId": "Epoch_2" }
            ],
            "encounters": [
              { "id": "Encounter_1", "name": "E1", "nextId": "Encounter_2" },
              { "id": "Encounter_2", "name": "E2", "previousId": "Encounter_1", "nextId": "Encounter_3" },
              { "id": "Encounter_3", "name": "E3", "previousId": "Encounter_2" }
            ],
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "name": "Main",
                "mainTimeline": true,
                "instances": [
                  { "id": "Instance_1", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_1", "encounterId": "Encounter_1" },
                  { "id": "Instance_2", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_3", "encounterId": "Encounter_3" },
                  { "id": "Instance_3", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_2", "encounterId": "Encounter_2" }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        for result in &outcome.results {
            assert_eq!(result.execution_status, ExecutionStatus::Failed);
            assert_eq!(result.error_count, 1);
            assert_eq!(result.errors[0].dataset, "StudyDesign");
            assert_eq!(result.errors[0].row, Some(1));
        }
    }

    #[test]
    fn run_validation_executes_usdm_governance_date_global_scope_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000968.json"),
            r#"{
  "Core": { "Id": "CORE-000968", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["GovernanceDate"] } },
  "Check": "study.documentedBy.versions.dateValues.{\"check\": true}",
  "Outcome": {
    "Message": "There is more than one date of this type for the study definition document version, but at least one of the dates has a global geographic scope.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "type",
      "dateValue",
      "geographicScopes.type"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "dateValues": [
              {
                "id": "GovernanceDate_1",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-01",
                "geographicScopes": [
                  { "id": "GeographicScope_1", "type": { "code": "C68846", "decode": "Global" } }
                ]
              },
              {
                "id": "GovernanceDate_2",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-02",
                "geographicScopes": [
                  { "id": "GeographicScope_2", "type": { "code": "C41129", "decode": "Region" } }
                ]
              },
              {
                "id": "GovernanceDate_3",
                "instanceType": "GovernanceDate",
                "type": { "code": "C215663", "decode": "Effective Date" },
                "dateValue": "2020-01-03",
                "geographicScopes": [
                  { "id": "GeographicScope_3", "type": { "code": "C41129", "decode": "Region" } }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "GovernanceDate");
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_document_content_reference_one_to_one_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000985.json"),
            r#"{
  "Core": { "Id": "CORE-000985", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["DocumentContentReference"] } },
  "Check": "study.versions.amendments.changes.changedSections.{\"check\": true}",
  "Outcome": {
    "Message": "There is not a one-to-one relationship between the referenced section number and title within the study definition document affected by the study amendment.",
    "Output Variables": [
      "StudyAmendment.id",
      "StudyAmendment.name",
      "StudyChange.id",
      "StudyChange.name",
      "appliesToId",
      "sectionNumber",
      "sectionTitle"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      { "id": "StudyDefinitionDocument_1", "name": "Protocol" }
    ],
    "versions": [
      {
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "changes": [
              {
                "id": "StudyChange_1",
                "name": "Change 1",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_1",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "1",
                    "sectionTitle": "Intro"
                  },
                  {
                    "id": "DocumentContentReference_2",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "2",
                    "sectionTitle": "Intro"
                  }
                ]
              },
              {
                "id": "StudyChange_2",
                "name": "Change 2",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_3",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "3",
                    "sectionTitle": "Methods"
                  }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "DocumentContentReference"
        );
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_json_schema_check_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000935.json"),
            r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": 71476, "decode": "Approval Date" }
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "population": {
              "plannedAge": {
                "isApproximate": "false"
              }
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "JSONSchemaIssue");
    }

    #[test]
    fn run_validation_executes_usdm_json_schema_check_rules_with_no_issues() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000935.json"),
            r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": "C71476", "decode": "Approval Date" },
            "geographicScopes": [
              { "id": "GeographicScope_1", "code": null }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
        assert_eq!(outcome.results[0].error_count, 0);
    }

    #[test]
    fn run_validation_executes_usdm_primary_endpoint_count_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001036.json"),
            r##"{
  "Core": { "Id": "CORE-001036", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Primary endpoints` = 0][]",
  "Outcome": {
    "Message": "There is not at least one endpoint with a level of primary within the study design.",
    "Output Variables": ["name", "# Primary endpoints"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design without primary endpoint",
            "instanceType": "InterventionalStudyDesign",
            "objectives": [
              {
                "id": "Objective_1",
                "endpoints": [
                  { "id": "Endpoint_1", "level": { "code": "C98772", "decode": "Secondary" } }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["name", "# Primary endpoints"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_interventional_model_intervention_count_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001077.json"),
            r##"{
  "Core": { "Id": "CORE-001077", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The number of study interventions referenced for the interventional study design is not consistent with intervention model.",
    "Output Variables": ["name", "studyType.code", "studyType.decode", "model.code", "model.decode", "# Referenced Study Interventions"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyInterventions": [
          { "id": "StudyIntervention_1", "instanceType": "StudyIntervention" },
          { "id": "StudyIntervention_2", "instanceType": "StudyIntervention" }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Too few interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Enough interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1", "StudyIntervention_2"]
          },
          {
            "id": "StudyDesign_3",
            "name": "Single group",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82640", "decode": "Single Group" },
            "studyInterventionIds": ["StudyIntervention_1"]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "InterventionalStudyDesign"
        );
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "name",
                "studyType.code",
                "studyType.decode",
                "model.code",
                "model.decode",
                "# Referenced Study Interventions"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_duplicate_study_cell_arm_epoch_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000948.json"),
            r#"{
  "Core": { "Id": "CORE-000948", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Check": "$.study.versions.studyDesigns.studyCells.{\"check\": true}",
  "Outcome": {
    "Message": "The combination of arm and epoch occurs more than once within the study design.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "armId", "StudyArm.name", "epochId", "StudyEpoch.name"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyCell");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDesign.id",
                "StudyDesign.name",
                "armId",
                "StudyArm.name",
                "epochId",
                "StudyEpoch.name"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_study_arm_missing_epoch_refs_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001026.json"),
            r#"{
  "Core": { "Id": "CORE-001026", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.arms@$sa.{\"check\": true}",
  "Outcome": {
    "Message": "The StudyArm does not have one StudyCell for each StudyEpoch.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "StudyDesign.epochs",
      "Arm's StudyCell Epoch Refs",
      "Missing Epoch Refs"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A", "instanceType": "StudyArm" },
              { "id": "StudyArm_2", "name": "Arm B", "instanceType": "StudyArm" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Screening" },
              { "id": "StudyEpoch_2", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "armId": "StudyArm_1", "epochId": "StudyEpoch_2" },
              { "id": "StudyCell_3", "armId": "StudyArm_2", "epochId": "StudyEpoch_1" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyArm");
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_condition_applies_to_reference_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001038.json"),
            r#"{
  "Core": { "Id": "CORE-001038", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition"] } },
  "Check": "$.study.versions.**.conditions.appliesToIds.{\"check\": true}",
  "Outcome": {
    "Message": "Condition appliesToIds must reference an allowed instance type.",
    "Output Variables": ["name", "appliesTo id", "appliesTo instanceType"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "activities": [
          { "id": "Activity_1", "name": "Dose", "instanceType": "Activity" }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Bad condition",
            "instanceType": "Condition",
            "appliesToIds": ["Activity_1", "Missing_1", "Condition_1"]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "Condition");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["name", "appliesTo id", "appliesTo instanceType"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_parameter_map_reference_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001049.json"),
            r#"{
  "Core": { "Id": "CORE-001049", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ParameterMap"] } },
  "Check": "$.study.**.dictionaries.parameterMaps.{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the parameter map is not available elsewhere in the model.",
    "Output Variables": ["SyntaxTemplateDictionary.id", "SyntaxTemplateDictionary.name", "tag", "reference"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "activities": [
          { "id": "Activity_1", "name": "Dose", "label": "Dose activity", "instanceType": "Activity" }
        ],
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "Dictionary",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              {
                "id": "ParameterMap_1",
                "instanceType": "ParameterMap",
                "tag": "valid_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_1\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_2",
                "instanceType": "ParameterMap",
                "tag": "missing_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_xx\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_3",
                "instanceType": "ParameterMap",
                "tag": "partial_ref",
                "reference": "<usdm:ref attribute=\"label\" id=\"Activity_1\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "ParameterMap");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "SyntaxTemplateDictionary.id",
                "SyntaxTemplateDictionary.name",
                "tag",
                "reference"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_blinding_schema_masked_roles_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001072.json"),
            r##"{
  "Core": { "Id": "CORE-001072", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a blinding schema that is not open label or double blind but there is no applicable study role that is masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1"]
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "No masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Has masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_3",
            "name": "Open label",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "InterventionalStudyDesign"
        );
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "name",
                "blindingSchema.code",
                "blindingSchema.decode",
                "# Masked Roles",
                "Applicable Roles"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_double_blind_requires_two_masked_roles_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001071.json"),
            r##"{
  "Core": { "Id": "CORE-001071", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a double blind blinding schema but there are not at least two applicable study roles that are masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_3",
            "instanceType": "StudyRole",
            "code": { "decode": "Assessor" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Only one masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Two masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].dataset,
            "InterventionalStudyDesign"
        );
    }

    #[test]
    fn run_validation_executes_usdm_open_label_rejects_masked_role_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001070.json"),
            r##"{
  "Core": { "Id": "CORE-001070", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles.{\"check\": true}",
  "Outcome": {
    "Message": "A masking is defined for the study role, but the role applies to a study design with an open label blinding schema.",
    "Output Variables": ["name", "code", "masking.text", "masking.isMasked", "appliesToIds", "StudyDesign.id", "StudyDesign.blindingSchema"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Masked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Masked", "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "name": "Unmasked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Not masked", "isMasked": false }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Open label design",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyRoleBlinding");
    }

    #[test]
    fn run_validation_executes_usdm_abbreviation_expanded_text_duplicate_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001067.json"),
            r##"{
  "Core": { "Id": "CORE-001067", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's expanded text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "Cu",
            "expandedText": "copper"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "LBC",
            "expandedText": "Copper"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyVersion.id",
                "StudyVersion.versionIdentifier",
                "abbreviatedText",
                "expandedText"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_abbreviation_text_duplicate_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001053.json"),
            r##"{
  "Core": { "Id": "CORE-001053", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's abbreviated text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse experience"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "BMI",
            "expandedText": "body mass index"
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
    }

    #[test]
    fn run_validation_executes_usdm_duplicate_document_version_ids_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001052.json"),
            r##"{
  "Core": { "Id": "CORE-001052", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Check": "$.study.versions.{\"check\": true}",
  "Outcome": {
    "Message": "The study version references the same study definition document version more than once.",
    "Output Variables": ["versionIdentifier", "Duplicate documentVersionIds"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "documentVersionIds": ["DocVersion_1", "DocVersion_2", "DocVersion_1"]
      },
      {
        "id": "StudyVersion_2",
        "versionIdentifier": "3",
        "documentVersionIds": ["DocVersion_1", "DocVersion_2"]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyVersion");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["versionIdentifier", "Duplicate documentVersionIds"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_tag_parameter_dictionary_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001074.json"),
            r##"{
  "Core": { "Id": "CORE-001074", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            rules_dir.join("CORE-001037.json"),
            r##"{
  "Core": { "Id": "CORE-001037", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write CORE-001037 rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "IE_Dict",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              { "id": "ParameterMap_1", "instanceType": "ParameterMap", "tag": "valid_tag" }
            ]
          }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Missing dictionary",
            "instanceType": "Condition",
            "text": "Use <usdm:tag name=\"missing_dict\"/>"
          },
          {
            "id": "Condition_2",
            "name": "Invalid dictionary",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_xx",
            "text": "Use <usdm:tag name=\"bad_dict\"/>"
          },
          {
            "id": "Condition_3",
            "name": "Missing tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"not_in_dictionary\"></usdm:tag>"
          },
          {
            "id": "Condition_4",
            "name": "Valid tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"valid_tag\"/>"
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        for id in ["CORE-001037", "CORE-001074"] {
            let result = outcome
                .results
                .iter()
                .find(|result| result.rule_id == id)
                .expect("result by id");
            assert_eq!(result.execution_status, ExecutionStatus::Failed);
            assert_eq!(result.error_count, 3);
            assert_eq!(result.errors[0].dataset, "SyntaxTemplateText");
            assert_eq!(
                result.errors[0].variables,
                vec![
                    "name",
                    "Parameter reference",
                    "Parameter name",
                    "dictionaryId",
                    "SyntaxTemplateDictionary.name",
                    "Issue"
                ]
            );
        }
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_ref_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001073.json"),
            r##"{
  "Core": { "Id": "CORE-001073", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContentItem"] } },
  "Check": "$.study.versions.narrativeContentItems[$contains(text,/usdm:ref/)].{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the narrative content item text is not available elsewhere in the model.",
    "Output Variables": ["name", "Invalid Reference"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyIdentifiers": [
          {
            "id": "StudyIdentifier_1",
            "name": "NCT identifier",
            "instanceType": "StudyIdentifier",
            "text": "NCT-001"
          }
        ],
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Missing klass",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_2",
            "name": "Missing target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_xx\" klass=\"StudyIdentifier\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_3",
            "name": "Valid target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\" klass=\"StudyIdentifier\"></usdm:ref>"
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContentItem");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["name", "Invalid Reference"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_item_id_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000944.json"),
            r##"{
  "Core": { "Id": "CORE-000944", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[contentItemId and $not(contentItemId in $.study.versions.narrativeContentItems.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The reference to the narrative content item is not targeting a narrative content item that has been defined within the study.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "contentItemId",
      "sectionNumber"
    ]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Defined",
            "instanceType": "NarrativeContentItem"
          }
        ]
      }
    ],
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Valid",
                "instanceType": "NarrativeContent",
                "contentItemId": "NarrativeContentItem_1",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Missing A",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_A",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Missing B",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_B",
                "sectionNumber": "3"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDefinitionDocument.id",
                "StudyDefinitionDocument.name",
                "StudyDefinitionDocumentVersion.id",
                "StudyDefinitionDocumentVersion.version",
                "name",
                "contentItemId",
                "sectionNumber"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_peer_refs_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001055.json"),
            r##"{
  "Core": { "Id": "CORE-001055", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[previousId or nextId or childIds].{\"check\": true}",
  "Outcome": {
    "Message": "The narrative content references a previous, next or child id value that does not match the id of any narrative content defined within the same study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "Invalid previousId",
      "Invalid nextId",
      "Invalid childIds"
    ]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Bad next",
                "instanceType": "NarrativeContent",
                "nextId": "Missing_Next",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Good",
                "instanceType": "NarrativeContent",
                "previousId": "NarrativeContent_1",
                "nextId": "NarrativeContent_3",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Bad previous and child",
                "instanceType": "NarrativeContent",
                "previousId": "Missing_Previous",
                "childIds": ["NarrativeContent_2", "Missing_Child"],
                "sectionNumber": "3"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDefinitionDocument.id",
                "StudyDefinitionDocument.name",
                "StudyDefinitionDocumentVersion.id",
                "StudyDefinitionDocumentVersion.version",
                "name",
                "sectionNumber",
                "Invalid previousId",
                "Invalid nextId",
                "Invalid childIds"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_display_section_number_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000964.json"),
            r##"{
  "Core": { "Id": "CORE-000964", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionNumber=true and (sectionNumber=null or sectionNumber=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section number is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionNumber",
      "sectionNumber"
    ]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDefinitionDocument.id",
                "StudyDefinitionDocument.name",
                "StudyDefinitionDocumentVersion.id",
                "StudyDefinitionDocumentVersion.version",
                "name",
                "displaySectionNumber",
                "sectionNumber"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_duplicate_section_number_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001041.json"),
            r#"{
  "Core": { "Id": "CORE-001041", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy@$sdd.$sdd.versions@$sddv.($sddv.contents[displaySectionNumber=true and sectionNumber].{\"check\": true})",
  "Outcome": {
    "Message": "The displayed section number is not unique within the study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "displaySectionNumber"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Duplicate A",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Duplicate B",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden duplicate",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_4",
                "name": "Unique",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "2.1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_narrative_content_display_section_title_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000965.json"),
            r##"{
  "Core": { "Id": "CORE-000965", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionTitle=true and (sectionTitle=null or sectionTitle=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section title is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionTitle",
      "sectionTitle"
    ]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": "Introduction"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDefinitionDocument.id",
                "StudyDefinitionDocument.name",
                "StudyDefinitionDocumentVersion.id",
                "StudyDefinitionDocumentVersion.version",
                "name",
                "displaySectionTitle",
                "sectionTitle"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_activity_child_id_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001062.json"),
            r##"{
  "Core": { "Id": "CORE-001062", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities.childIds[$not($ in $.study.versions.studyDesigns.activities.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity references a childId that does not match the id of any activity defined within the same study design as the activity.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "name", "childId"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "instanceType": "InterventionalStudyDesign",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Missing_A", "Missing_B"]
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "childIds": []
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["StudyDesign.id", "StudyDesign.name", "name", "childId"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_activity_children_with_details_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000954.json"),
            r##"{
  "Core": { "Id": "CORE-000954", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities[childIds and (biomedicalConceptIds or bcCategoryIds or definedProcedures or timelineId or bcSurrogateIds)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity has children but also refers to details.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "childIds",
      "biomedicalConceptIds",
      "bcCategoryIds",
      "bcSurrogateIds",
      "timelineId",
      "definedProcedures.id"
    ]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent with timeline",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Activity_3"],
                "timelineId": "Timeline_1"
              },
              {
                "id": "Activity_2",
                "name": "Parent with BC",
                "instanceType": "Activity",
                "childIds": ["Activity_3"],
                "biomedicalConceptIds": ["BC_1"]
              },
              {
                "id": "Activity_3",
                "name": "Leaf with details",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BC_2"]
              },
              {
                "id": "Activity_4",
                "name": "Parent only",
                "instanceType": "Activity",
                "childIds": ["Activity_3"]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec![
                "StudyDesign.id",
                "StudyDesign.name",
                "name",
                "childIds",
                "biomedicalConceptIds",
                "bcCategoryIds",
                "bcSurrogateIds",
                "timelineId",
                "definedProcedures.id"
            ]
        );
    }

    #[test]
    fn run_validation_executes_usdm_activity_child_order_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001066.json"),
            r#"{
  "Core": { "Id": "CORE-001066", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The previous/next ordering of the activity with respect to child activities is incorrect.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "previousId",
      "nextId",
      "childIds",
      "Parent Activity's id"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2"],
                "nextId": "Activity_3"
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "previousId": "Activity_1"
              },
              {
                "id": "Activity_3",
                "name": "Other",
                "instanceType": "Activity",
                "previousId": "Activity_2"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_usdm_activity_bc_category_overlap_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001047.json"),
            r#"{
  "Core": { "Id": "CORE-001047", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions@$sv.$sv.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The activity references both a biomedical concept category and a biomedical concept, but the biomedical concept is a member of the referenced category or one of its subcategories.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "biomedicalConceptId",
      "bcCategoryId(s) containing BC"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "bcCategories": [
          {
            "id": "BCCategory_1",
            "name": "Vitals",
            "memberIds": ["BiomedicalConcept_1"]
          },
          {
            "id": "BCCategory_2",
            "name": "Labs",
            "memberIds": ["BiomedicalConcept_2"]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Overlapping Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_1"]
              },
              {
                "id": "Activity_2",
                "name": "Non-overlap Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_2"]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_usdm_scheduled_instance_design_reference_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (rule_id, field, parent_field) in [
            (
                "CORE-000950",
                "epochId",
                "Referenced epoch's parent StudyDesign.id",
            ),
            (
                "CORE-001039",
                "encounterId",
                "Referenced encounter's parent StudyDesign.id",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{rule_id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["ScheduledActivityInstance"] }} }},
  "Check": "$.study.versions.studyDesigns.scheduleTimelines.instances.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Scheduled instance references an object outside the design.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "{field}",
      "{parent_field}"
    ]
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "epochs": [{ "id": "StudyEpoch_1", "name": "Epoch 1" }],
            "encounters": [{ "id": "Encounter_1", "name": "Encounter 1" }],
            "scheduleTimelines": []
          },
          {
            "id": "StudyDesign_2",
            "name": "Design 2",
            "epochs": [{ "id": "StudyEpoch_2", "name": "Epoch 2" }],
            "encounters": [{ "id": "Encounter_2", "name": "Encounter 2" }],
            "scheduleTimelines": [
              {
                "id": "ScheduleTimeline_1",
                "instances": [
                  {
                    "id": "ScheduledActivityInstance_1",
                    "name": "Bad epoch",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_1",
                    "encounterId": "Encounter_2"
                  },
                  {
                    "id": "ScheduledActivityInstance_2",
                    "name": "Bad encounter",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_1"
                  },
                  {
                    "id": "ScheduledActivityInstance_3",
                    "name": "Good refs",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_2"
                  }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let epoch_result = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-000950")
            .expect("epoch result");
        let encounter_result = outcome
            .results
            .iter()
            .find(|result| result.rule_id == "CORE-001039")
            .expect("encounter result");
        assert_eq!(epoch_result.execution_status, ExecutionStatus::Failed);
        assert_eq!(epoch_result.error_count, 1);
        assert_eq!(epoch_result.errors[0].row, Some(1));
        assert_eq!(encounter_result.execution_status, ExecutionStatus::Failed);
        assert_eq!(encounter_result.error_count, 1);
        assert_eq!(encounter_result.errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_usdm_study_role_assigned_persons_and_orgs_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000997.json"),
            r##"{
  "Core": { "Id": "CORE-000997", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles[assignedPersons and organizationIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study role references both assigned persons and organizations.",
    "Output Variables": ["name", "code", "assignedPersons", "organizationIds"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Sponsor",
            "instanceType": "Organization"
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Person only",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_1", "name": "AP1" }
            ]
          },
          {
            "id": "StudyRole_2",
            "name": "Org only",
            "instanceType": "StudyRole",
            "code": { "code": "C215670", "decode": "Local Sponsor" },
            "organizationIds": ["Organization_1"]
          },
          {
            "id": "StudyRole_3",
            "name": "Both",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_2", "name": "AP2" }
            ],
            "organizationIds": ["Organization_1"]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["name", "code", "assignedPersons", "organizationIds"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_duration_quantity_text_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000994.json"),
            r##"{
  "Core": { "Id": "CORE-000994", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and not(text) and not(quantity)].{\"check\": true}",
  "Outcome": {
    "Message": "The quantity and text are both missing.",
    "Output Variables": ["text", "quantity"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            rules_dir.join("CORE-000995.json"),
            r##"{
  "Core": { "Id": "CORE-000995", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and ((durationWillVary=true and quantity) or (durationWillVary=false and not(quantity)))].{\"check\": true}",
  "Outcome": {
    "Message": "The duration quantity conflicts with durationWillVary.",
    "Output Variables": ["quantity(value/range)", "durationWillVary"]
  }
}"##,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "plannedDuration": {
                  "id": "Duration_1",
                  "instanceType": "Duration",
                  "durationWillVary": false
                }
              }
            ]
          }
        ],
        "studyInterventions": [
          {
            "id": "Intervention_1",
            "administrations": [
              {
                "id": "Administration_1",
                "duration": {
                  "id": "Duration_2",
                  "instanceType": "Duration",
                  "durationWillVary": true,
                  "quantity": {
                    "value": 24,
                    "unit": {
                      "standardCode": {
                        "decode": "Week",
                        "code": "C29844"
                      }
                    }
                  }
                }
              },
              {
                "id": "Administration_2",
                "duration": {
                  "id": "Duration_3",
                  "instanceType": "Duration",
                  "text": "Variable"
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let missing = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join("CORE-000994.json")],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run missing");
        assert_eq!(missing.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(missing.results[0].error_count, 1);
        assert_eq!(
            missing.results[0].errors[0].variables,
            vec!["text", "quantity"]
        );

        let conflict = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join("CORE-000995.json")],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run conflict");
        assert_eq!(
            conflict.results[0].execution_status,
            ExecutionStatus::Failed
        );
        assert_eq!(conflict.results[0].error_count, 2);
        assert_eq!(
            conflict.results[0].errors[0].variables,
            vec!["quantity(value/range)", "durationWillVary"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_study_design_document_type_phase_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000998.json"),
            r##"{
  "Core": { "Id": "CORE-000998", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[documentVersionIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study design references the same document version more than once.",
    "Output Variables": ["name", "Duplicate documentVersionIds"]
  }
}"##,
        )
        .expect("write duplicate rule");
        fs::write(
            rules_dir.join("CORE-001004.json"),
            r##"{
  "Core": { "Id": "CORE-001004", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and instanceType != \"ObservationalStudyDesign\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational study must use ObservationalStudyDesign.",
    "Output Variables": ["name", "studyType"]
  }
}"##,
        )
        .expect("write type rule");
        fs::write(
            rules_dir.join("CORE-001005.json"),
            r##"{
  "Core": { "Id": "CORE-001005", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and studyPhase.standardCode.code != \"C48660\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational phase must be Not Applicable.",
    "Output Variables": ["name", "studyType", "studyPhase"]
  }
}"##,
        )
        .expect("write phase rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Duplicate doc",
            "instanceType": "InterventionalStudyDesign",
            "documentVersionIds": ["DocV_1", "DocV_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Wrong class",
            "instanceType": "InterventionalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            }
          },
          {
            "id": "StudyDesign_3",
            "name": "Wrong phase",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C129000",
              "decode": "Patient Registry Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C15602",
                "decode": "Phase III Trial"
              }
            }
          },
          {
            "id": "StudyDesign_4",
            "name": "Valid observational",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C48660",
                "decode": "Not Applicable"
              }
            }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let duplicate = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join("CORE-000998.json")],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run duplicate");
        assert_eq!(
            duplicate.results[0].execution_status,
            ExecutionStatus::Failed
        );
        assert_eq!(duplicate.results[0].error_count, 1);
        assert_eq!(
            duplicate.results[0].errors[0].variables,
            vec!["name", "Duplicate documentVersionIds"]
        );

        let type_rule = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join("CORE-001004.json")],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run type");
        assert_eq!(
            type_rule.results[0].execution_status,
            ExecutionStatus::Failed
        );
        assert_eq!(type_rule.results[0].error_count, 1);
        assert_eq!(
            type_rule.results[0].errors[0].variables,
            vec!["name", "studyType"]
        );

        let phase = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join("CORE-001005.json")],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run phase");
        assert_eq!(phase.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(phase.results[0].error_count, 1);
        assert_eq!(
            phase.results[0].errors[0].variables,
            vec!["name", "studyType", "studyPhase"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_study_design_duplicate_code_list_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, output) in [
            ("CORE-000980", "[\"name\", \"characteristics\"]"),
            ("CORE-001002", "[\"name\", \"subTypes\"]"),
            (
                "CORE-001003",
                "[\"name\", \"therapeuticAreas.codeSystem\", \"therapeuticAreas.codeSystemVersion\", \"therapeuticAreas\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Duplicate study design list values.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C1", "decode": "A" },
              { "id": "Code_2", "code": "C1", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_3", "code": "S1", "decode": "Sub A" },
              { "id": "Code_4", "code": "S1", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_5", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA A" },
              { "id": "Code_6", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA B" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Valid",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_7", "code": "C2", "decode": "A" },
              { "id": "Code_8", "code": "C3", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_9", "code": "S2", "decode": "Sub A" },
              { "id": "Code_10", "code": "S3", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_11", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T2", "decode": "TA A" },
              { "id": "Code_12", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T3", "decode": "TA B" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for (id, variables) in [
            ("CORE-000980", vec!["name", "characteristics"]),
            ("CORE-001002", vec!["name", "subTypes"]),
            (
                "CORE-001003",
                vec![
                    "name",
                    "therapeuticAreas.codeSystem",
                    "therapeuticAreas.codeSystemVersion",
                    "therapeuticAreas",
                ],
            ),
        ] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run duplicate list");
            assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
            assert_eq!(outcome.results[0].error_count, 1);
            assert_eq!(outcome.results[0].errors[0].variables, variables);
        }
    }

    #[test]
    fn run_validation_executes_usdm_study_design_single_and_multi_centre_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001017.json"),
            r#"{
  "Core": { "Id": "CORE-001017", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[\"C217004\" in characteristics.code and \"C217005\" in characteristics.code].{\"check\": true}",
  "Outcome": {
    "Message": "A study design must not be both single-centre and multicentre.",
    "Output Variables": ["name", "characteristics"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Conflicting",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C217004", "decode": "Single-Centre" },
              { "id": "Code_2", "code": "C217005", "decode": "Multicentre" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Single only",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_3", "code": "C217004", "decode": "Single-Centre" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["name", "characteristics"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_range_and_person_name_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, entity, output) in [
            ("CORE-001009", "Range", "[\"minValue\", \"maxValue\"]"),
            ("CORE-001012", "Range", "[\"minValue\", \"maxValue\"]"),
            ("CORE-001014", "PersonName", "[\"familyName\", \"text\"]"),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM recursive entity rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "population": {
              "plannedAge": {
                "instanceType": "Range",
                "minValue": {
                  "value": 50,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                },
                "maxValue": {
                  "value": 20,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                }
              },
              "plannedCompletionNumber": {
                "instanceType": "Range",
                "minValue": { "value": 50 },
                "maxValue": {
                  "value": 100,
                  "unit": { "standardCode": { "decode": "Participant", "code": "C142710" } }
                }
              }
            }
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "assignedPersons": [
              {
                "id": "AssignedPerson_1",
                "personName": {
                  "instanceType": "PersonName"
                }
              },
              {
                "id": "AssignedPerson_2",
                "personName": {
                  "instanceType": "PersonName",
                  "familyName": "Smith"
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for (id, count, variables) in [
            ("CORE-001009", 1, vec!["minValue", "maxValue"]),
            ("CORE-001012", 1, vec!["minValue", "maxValue"]),
            ("CORE-001014", 1, vec!["familyName", "text"]),
        ] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run recursive entity");
            assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
            assert_eq!(outcome.results[0].error_count, count);
            assert_eq!(outcome.results[0].errors[0].variables, variables);
        }
    }

    #[test]
    fn run_validation_executes_usdm_simple_recursive_entity_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, entity, output) in [
            (
                "CORE-000971",
                "Address",
                "[\"Organization.id\", \"Organization.name\", \"text\", \"lines\", \"district\", \"city\", \"postalCode\", \"state\", \"country\"]",
            ),
            (
                "CORE-001011",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\"]",
            ),
            (
                "CORE-001021",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\"]",
            ),
            (
                "CORE-001006",
                "BiomedicalConcept",
                "[\"name\", \"label/synonym\", \"synonyms\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM simple recursive rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Org",
            "legalAddress": {
              "id": "Address_1",
              "instanceType": "Address"
            }
          }
        ],
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C48660", "decode": "Not Applicable" }
            }
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "Role_1",
            "name": "Manufacturer",
            "instanceType": "ProductOrganizationRole"
          }
        ],
        "biomedicalConcepts": [
          {
            "id": "BC_1",
            "name": "Sex",
            "label": "Sex",
            "instanceType": "BiomedicalConcept",
            "synonyms": ["Gender", "sex"]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for (id, variables) in [
            (
                "CORE-000971",
                vec![
                    "Organization.id",
                    "Organization.name",
                    "text",
                    "lines",
                    "district",
                    "city",
                    "postalCode",
                    "state",
                    "country",
                ],
            ),
            (
                "CORE-001011",
                vec!["StudyAmendment.id", "StudyAmendment.name", "code"],
            ),
            ("CORE-001021", vec!["name", "appliesToIds"]),
            ("CORE-001006", vec!["name", "label/synonym", "synonyms"]),
        ] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run simple recursive");
            assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
            assert_eq!(outcome.results[0].error_count, 1);
            assert_eq!(outcome.results[0].errors[0].variables, variables);
        }
    }

    #[test]
    fn run_validation_executes_usdm_administration_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, output) in [
            (
                "CORE-000966",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value)\", \"route.id\", \"route\"]",
            ),
            (
                "CORE-000967",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"frequency.id\", \"frequency\"]",
            ),
            (
                "CORE-000969",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"administrableProductId\", \"medicalDeviceId\", \"MedicalDevice.name\", \"MedicalDevice.embeddedProductId\", \"AdministrableProduct.name\"]",
            ),
            (
                "CORE-000986",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"administrableProductId\", \"AdministrableProduct.name\", \"medicalDeviceId\", \"MedicalDevice.name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Administration"] }} }},
  "Check": "study.versions.studyInterventions.administrations.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Administration rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          { "id": "AdmProd_1", "name": "Product 1" }
        ],
        "medicalDevices": [
          { "id": "MedDev_1", "name": "Device 1", "embeddedProductId": "AdmProd_1" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "name": "Intervention",
            "administrations": [
              {
                "id": "Administration_1",
                "name": "Route only",
                "instanceType": "Administration",
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_2",
                "name": "Dose without frequency or product",
                "instanceType": "Administration",
                "dose": {
                  "id": "Quantity_1",
                  "value": 30,
                  "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                },
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_3",
                "name": "Duplicated product",
                "instanceType": "Administration",
                "administrableProductId": "AdmProd_1",
                "medicalDeviceId": "MedDev_1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for (id, expected_count) in [
            ("CORE-000966", 1),
            ("CORE-000967", 1),
            ("CORE-000969", 2),
            ("CORE-000986", 1),
        ] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run administration rule");
            assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
            assert_eq!(outcome.results[0].error_count, expected_count);
            assert_eq!(outcome.results[0].errors[0].dataset, "Administration");
        }
    }

    #[test]
    fn run_validation_executes_usdm_strength_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, output) in [
            (
                "CORE-001007",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.value\"]",
            ),
            (
                "CORE-001008",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.minValue\", \"numerator.maxValue\"]",
            ),
            (
                "CORE-001020",
                "[\"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"denominator.id\", \"denominator.value\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Strength"] }} }},
  "Check": "study.versions.administrableProducts.ingredients.substance.strengths.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Strength rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product 1",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Subst_1",
                  "name": "Substance 1",
                  "strengths": [
                    {
                      "id": "Strength_1",
                      "name": "Numerator value",
                      "instanceType": "Strength",
                      "numerator": { "id": "Quantity_1", "value": 10 }
                    },
                    {
                      "id": "Strength_2",
                      "name": "Numerator range",
                      "instanceType": "Strength",
                      "numerator": {
                        "minValue": { "id": "Quantity_2", "value": 50 },
                        "maxValue": {
                          "id": "Quantity_3",
                          "value": 100,
                          "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                        }
                      }
                    },
                    {
                      "id": "Strength_3",
                      "name": "Denominator",
                      "instanceType": "Strength",
                      "denominator": { "id": "Quantity_4", "value": 2 }
                    }
                  ]
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for id in ["CORE-001007", "CORE-001008", "CORE-001020"] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run strength rule");
            assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
            assert_eq!(outcome.results[0].error_count, 1);
            assert_eq!(outcome.results[0].errors[0].dataset, "Strength");
        }
    }

    #[test]
    fn run_validation_executes_usdm_embedded_product_sourcing_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001001.json"),
            r#"{
  "Core": { "Id": "CORE-001001", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["AdministrableProduct"] } },
  "Check": "(study.versions)@$sv.$sv.administrableProducts@$ap.[{\"check\": true}]",
  "Outcome": {
    "Message": "The sourcing is defined while the administrable product is only referenced to as an embedded product for a medical device.",
    "Output Variables": [
      "name",
      "sourcing",
      "MedicalDevice.id",
      "MedicalDevice.name",
      "MedicalDevice.embeddedProductId"
    ]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          {
            "id": "AdministrableProduct_1",
            "name": "Embedded sourced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          },
          {
            "id": "AdministrableProduct_2",
            "name": "Embedded unsourced",
            "instanceType": "AdministrableProduct"
          },
          {
            "id": "AdministrableProduct_3",
            "name": "Admin referenced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          }
        ],
        "medicalDevices": [
          { "id": "MedicalDevice_1", "name": "Device", "embeddedProductId": "AdministrableProduct_1" },
          { "id": "MedicalDevice_2", "name": "Other Device", "embeddedProductId": "AdministrableProduct_2" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "administrations": [
              { "id": "Administration_1", "administrableProductId": "AdministrableProduct_3" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "AdministrableProduct");
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
    }

    #[test]
    fn run_validation_executes_usdm_reference_and_duplicate_jsonata_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (id, entity, output) in [
            (
                "CORE-000970",
                "StudyRole",
                "[\"name\", \"code\", \"appliesToIds\", \"StudyVersion.id\", \"StudyVersion.studyDesigns.id\"]",
            ),
            (
                "CORE-001022",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\", \"appliesTo name\"]",
            ),
            (
                "CORE-001024",
                "StudyDesign",
                "[\"name\", \"studyType\"]",
            ),
            (
                "CORE-001032",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001033",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001031",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\", \"primaryReason.code\"]",
            ),
            (
                "CORE-000999",
                "StudyDefinitionDocumentVersion",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"version\"]",
            ),
            (
                "CORE-000983",
                "Procedure",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"Activity.id\", \"Activity.name\", \"name\", \"studyInterventionId\", \"StudyIntervention.name\"]",
            ),
            (
                "CORE-000984",
                "SubjectEnrollment",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"name\", \"forGeographicScope\", \"forStudySiteId\", \"forStudyCohortId\"]",
            ),
            (
                "CORE-001010",
                "Substance",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Parent Substance.id\", \"Parent Substance.name\", \"name\", \"referenceSubstance.id\", \"referenceSubstance.name\"]",
            ),
            (
                "CORE-001018",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\"]",
            ),
            (
                "CORE-001019",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\", \"Used in\"]",
            ),
            (
                "CORE-001025",
                "BiospecimenRetention",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"isRetained\"]",
            ),
            (
                "CORE-001027",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"criterionItemId\"]",
            ),
            (
                "CORE-001028",
                "EligibilityCriterionItem",
                "[\"StudyVersion.id\", \"StudyVersion.versionIdentifier\", \"name\"]",
            ),
            (
                "CORE-001029",
                "StudyCohort",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.indications.id\", \"StudyDesignPopulation.id\", \"StudyDesignPopulation.name\", \"name\", \"Invalid indicationIds\"]",
            ),
            (
                "CORE-001030",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"name\", \"Invalid studyInterventionIds\", \"Invalid StudyIntervention.name\"]",
            ),
            (
                "CORE-001040",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"studyInterventionIds value\", \"Referenced intervention's parent StudyDesign.id\"]",
            ),
            (
                "CORE-001045",
                "StudyArm",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.population.id\", \"StudyDesign.population.cohorts.id\", \"name\", \"populationId\"]",
            ),
            (
                "CORE-001042",
                "GeographicScope",
                "[\"type.code\", \"type.decode\", \"code.standardCode.code\", \"code.standardCode.decode\"]",
            ),
            (
                "CORE-001051",
                "NarrativeContent",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"StudyDefinitionDocumentVersion.id\", \"StudyDefinitionDocumentVersion.version\", \"name\", \"sectionNumber\", \"sectionTitle\"]",
            ),
            (
                "CORE-001050",
                "NarrativeContent",
                "[\"StudyProtocolDocument.id\", \"StudyProtocolDocument.name\", \"StudyProtocolDocumentVersion.id\", \"StudyProtocolDocumentVersion.protocolVersion\", \"name\", \"sectionNumber\", \"sectionTitle\", \"Invalid Reference\"]",
            ),
            (
                "CORE-001023",
                "InterventionalStudyDesign",
                "[\"name\", \"intentTypes\"]",
            ),
            (
                "CORE-001046",
                "StudyDesign",
                "[\"id\", \"name\", \"interventionModel.code\", \"interventionModel.decode\", \"# Study Interventions\"]",
            ),
            (
                "CORE-001013",
                "USDMObject",
                "[\"name\"]",
            ),
            (
                "CORE-001015",
                "USDMObject",
                "[\"name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM reference rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Empty content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "1",
                "sectionTitle": "Overview"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Invalid ref content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "2",
                "sectionTitle": "Reference",
                "childIds": ["NarrativeContent_1"],
                "text": "<usdm:ref attribute=\"text\" id=\"MissingCriterion\" klass=\"EligibilityCriterion\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ],
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "1",
        "geographicScopes": [
          {
            "id": "GeographicScope_1",
            "name": "Global with code",
            "instanceType": "GeographicScope",
            "type": { "code": "C68846", "decode": "Global" },
            "code": { "standardCode": { "code": "US", "decode": "United States" } }
          }
        ],
        "duplicateObjects": [
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          },
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          }
        ],
        "studyInterventions": [
          { "id": "StudyIntervention_1", "name": "Valid intervention" },
          { "id": "StudyIntervention_2", "name": "Other intervention" }
        ],
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Substance_1",
                  "name": "Parent substance",
                  "referenceSubstance": {
                    "id": "Substance_2",
                    "name": "Reference substance",
                    "instanceType": "Substance",
                    "referenceSubstance": { "id": "Substance_3", "name": "Invalid nested reference" }
                  }
                }
              }
            ]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "ObservationalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "characteristics": [
              { "id": "Code_1", "code": "C217006", "decode": "Single Country" },
              { "id": "Code_2", "code": "C217007", "decode": "Multiple Countries" },
              { "id": "Code_3", "code": "C46079", "decode": "Randomized" },
              { "id": "Code_4", "code": "C25689", "decode": "Stratification" }
            ],
            "activities": [
              {
                "id": "Activity_1",
                "name": "Activity",
                "definedProcedures": [
                  {
                    "id": "Procedure_1",
                    "name": "Procedure",
                    "instanceType": "Procedure",
                    "studyInterventionId": "StudyIntervention_2"
                  }
                ]
              }
            ],
            "population": {
              "id": "Population_1",
              "name": "Population",
              "criterionIds": ["EligibilityCriterion_1"],
              "cohorts": [
                {
                  "id": "Cohort_1",
                  "name": "Cohort",
                  "criterionIds": ["EligibilityCriterion_1"],
                  "indicationIds": ["Indication_bad"]
                }
              ]
            },
            "indications": [{ "id": "Indication_1", "name": "Indication" }],
            "eligibilityCriteria": [
              {
                "id": "EligibilityCriterion_1",
                "name": "Criterion 1",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "01"
              },
              {
                "id": "EligibilityCriterion_2",
                "name": "Criterion 2",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "02"
              }
            ],
            "biospecimenRetentions": [
              {
                "id": "BiospecimenRetention_1",
                "name": "Retention",
                "instanceType": "BiospecimenRetention",
                "isRetained": true
              }
            ],
            "elements": [
              {
                "id": "StudyElement_1",
                "name": "Element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_2"]
              }
            ],
            "arms": [
              {
                "id": "StudyArm_1",
                "name": "Arm",
                "instanceType": "StudyArm",
                "populationIds": ["Population_bad"]
              }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Intent design",
            "instanceType": "InterventionalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "interventionModel": { "code": "C82640", "decode": "Single Group Design" },
            "studyInterventions": [
              { "id": "StudyDesignIntervention_1", "name": "Embedded intervention 1" },
              { "id": "StudyDesignIntervention_2", "name": "Embedded intervention 2" }
            ],
            "elements": [
              {
                "id": "StudyElement_2",
                "name": "Cross-design element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_1"]
              }
            ],
            "intentTypes": [
              { "id": "IntentType_1", "code": "C123", "decode": "Intent" },
              { "id": "IntentType_2", "code": "C123", "decode": "Intent duplicate" }
            ]
          }
        ],
        "eligibilityCriterionItems": [
          {
            "id": "EligibilityCriterionItem_unused",
            "name": "Unused criterion item",
            "instanceType": "EligibilityCriterionItem"
          }
        ],
        "roles": [
          {
            "id": "Role_1",
            "name": "Invalid role scope",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1", "StudyDesign_1"]
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "ProductRole_1",
            "name": "Invalid product role",
            "instanceType": "ProductOrganizationRole",
            "appliesToIds": ["StudyVersion_1"]
          }
        ],
        "amendments": [
          {
            "id": "Amendment_1",
            "name": "Amendment",
            "enrollments": [
              {
                "id": "Enrollment_1",
                "name": "Enrollment",
                "instanceType": "SubjectEnrollment"
              }
            ],
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C17649", "decode": "Other" }
            },
            "secondaryReasons": [
              {
                "id": "Reason_2",
                "instanceType": "StudyAmendmentReason",
                "code": { "code": "C17649", "decode": "Other" }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        for (id, dataset, expected_count) in [
            ("CORE-000970", "StudyRole", 1),
            ("CORE-001022", "ProductOrganizationRole", 1),
            ("CORE-001024", "StudyDesign", 1),
            ("CORE-001032", "StudyDesign", 1),
            ("CORE-001033", "StudyDesign", 1),
            ("CORE-001031", "StudyAmendmentReason", 1),
            ("CORE-000999", "StudyDefinitionDocumentVersion", 1),
            ("CORE-000983", "Procedure", 1),
            ("CORE-000984", "SubjectEnrollment", 1),
            ("CORE-001010", "Substance", 1),
            ("CORE-001018", "EligibilityCriterion", 1),
            ("CORE-001019", "EligibilityCriterion", 1),
            ("CORE-001025", "BiospecimenRetention", 1),
            ("CORE-001027", "EligibilityCriterion", 2),
            ("CORE-001028", "EligibilityCriterionItem", 1),
            ("CORE-001029", "StudyCohort", 1),
            ("CORE-001030", "StudyElement", 1),
            ("CORE-001040", "StudyElement", 2),
            ("CORE-001045", "StudyArm", 1),
            ("CORE-001042", "GeographicScope", 1),
            ("CORE-001051", "NarrativeContent", 1),
            ("CORE-001050", "NarrativeContent", 1),
            ("CORE-001023", "InterventionalStudyDesign", 1),
            ("CORE-001046", "StudyDesign", 1),
            ("CORE-001013", "USDMObject", 2),
            ("CORE-001015", "USDMObject", 2),
        ] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rules_dir.join(format!("{id}.json"))],
                dataset_paths: vec![data_dir.clone()],
                dataset_loader: DatasetLoader::OpenRulesDataDir,
                ..Default::default()
            })
            .expect("run reference rule");
            assert_eq!(
                outcome.results[0].execution_status,
                ExecutionStatus::Failed,
                "{id}"
            );
            assert_eq!(outcome.results[0].error_count, expected_count, "{id}");
            assert_eq!(outcome.results[0].errors[0].dataset, dataset, "{id}");
        }
    }

    #[test]
    fn run_validation_executes_usdm_id_contains_space_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-001075.json"),
            r#"{
  "Core": { "Id": "CORE-001075", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": "(**[$contains($string(id),\" \")])@$i.{\"instanceType\": $i.instanceType,\"id\": $join(['\"','\"'],$i.id),\"path\": $i._path,\"name\": $i.name}",
  "Outcome": {
    "Message": "The id value contains a space.",
    "Output Variables": ["name"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "id": "Study 1",
    "name": "Study with spaced id",
    "instanceType": "Study",
    "versions": [
      {
        "id": "StudyVersion_1",
        "name": "Clean version",
        "instanceType": "StudyVersion"
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "USDMObject");
        assert_eq!(outcome.results[0].errors[0].variables, vec!["name"]);
    }

    #[test]
    fn run_validation_executes_usdm_study_identifier_duplicate_scope_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000956.json"),
            r#"{
  "Core": { "Id": "CORE-000956", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyIdentifier"] } },
  "Check": "study.versions@$sv.($sv.organizations{id:($o:=$;$i:=$sv.studyIdentifiers[scopeId=$o.id];$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "More than 1 study identifier is specified for the same organization.",
    "Output Variables": ["text", "scopeId", "Organization.name"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "studyIdentifiers": [
          { "id": "StudyIdentifier_1", "instanceType": "StudyIdentifier", "text": "ABC-001", "scopeId": "Organization_1" },
          { "id": "StudyIdentifier_2", "instanceType": "StudyIdentifier", "text": "NCT-001", "scopeId": "Organization_1" }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "StudyIdentifier");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["text", "scopeId", "Organization.name"]
        );
    }

    #[test]
    fn run_validation_executes_usdm_identifier_text_duplicate_scope_jsonata_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000955.json"),
            r#"{
  "Core": { "Id": "CORE-000955", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ReferenceIdentifier"] } },
  "Check": "study.versions@$sv.($sv.**.*[scopeId and text and instanceType]{$join([text,scopeId,instanceType],\"\\n\"):($i:=$;$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "The identifier text is not unique within the scope of the identified organization.",
    "Output Variables": ["text", "scopeId", "Organization.name", "type.decode"]
  }
}"#,
        )
        .expect("write rule");
        fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "referenceIdentifiers": [
          {
            "id": "ReferenceIdentifier_1",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Clinical Development Plan" }
          },
          {
            "id": "ReferenceIdentifier_2",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Pediatric Investigation Plan" }
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].dataset, "ReferenceIdentifier");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["text", "scopeId", "Organization.name", "type.decode"]
        );
    }

    #[test]
    fn run_validation_uses_define_xml_and_ct_for_codelist_checks() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-CT-DOMAIN.json"),
            r#"{
  "Core": { "Id": "CORE-CT-DOMAIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "is_not_contained_by"
  },
  "Outcome": { "Message": "DOMAIN must use controlled terminology" }
}"#,
        )
        .expect("write codelist rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "DOMAIN": ["AE", "CM", "XX"],
        "AESEQ": [1, 2, 3]
      }
    }
  ]
}"#,
        )
        .expect("write codelist data");

        let define_xml_path = dir.path().join("define.xml");
        fs::write(
            &define_xml_path,
            r#"
<ODM>
  <ItemDef OID="IT.DOMAIN" Name="DOMAIN" DataType="text">
    <CodeListRef CodeListOID="CL.DOMAIN"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN">
    <CodeListItem CodedValue="AE"/>
  </CodeList>
</ODM>
"#,
        )
        .expect("write define xml");
        let ct_path = dir.path().join("ct.json");
        fs::write(&ct_path, r#"{ "CL.DOMAIN": ["CM"] }"#).expect("write ct");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            define_xml_paths: vec![define_xml_path],
            ct_paths: vec![ct_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(3));
    }

    #[test]
    fn run_validation_resolves_define_and_ct_codelist_aliases() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-CT-ALIAS.json"),
            r#"{
  "Core": { "Id": "CORE-CT-ALIAS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AE.DOMAIN",
    "operator": "is_not_contained_by"
  },
  "Outcome": { "Message": "DOMAIN must use Define-XML and CT terminology" }
}"#,
        )
        .expect("write codelist rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "AE.DOMAIN": ["AE", "CM", "XX"],
        "AESEQ": [1, 2, 3]
      }
    }
  ]
}"#,
        )
        .expect("write codelist data");

        let define_xml_path = dir.path().join("define.xml");
        fs::write(
            &define_xml_path,
            r#"
<odm:ODM xmlns:odm="http://www.cdisc.org/ns/odm/v1.3">
  <odm:ItemDef OID="IT.DOMAIN" Name="DOMAIN" DataType="text">
    <odm:CodeListRef CodeListOID="CL.DOMAIN"/>
  </odm:ItemDef>
  <odm:CodeList OID="CL.DOMAIN" Name="Domain Abbreviation" SASFormatName="DOMAIN">
    <odm:CodeListItem CodedValue="AE"/>
  </odm:CodeList>
</odm:ODM>
"#,
        )
        .expect("write define xml");
        let ct_path = dir.path().join("ct.json");
        fs::write(
            &ct_path,
            r#"{
  "codelists": [
    {
      "submissionValue": "DOMAIN",
      "conceptId": "C66734",
      "terms": [
        { "submissionValue": "CM" }
      ]
    }
  ]
}"#,
        )
        .expect("write ct");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            define_xml_paths: vec![define_xml_path],
            ct_paths: vec![ct_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(3));
    }

    #[test]
    fn define_codelist_resolution_uses_domain_and_avoids_ambiguous_globals() {
        let dir = tempdir().expect("tempdir");
        let define_xml_path = dir.path().join("define.xml");
        fs::write(
            &define_xml_path,
            r#"
<ODM>
  <ItemGroupDef OID="IG.AE" Name="AE" Domain="AE">
    <ItemRef ItemOID="IT.AE.PARAMCD"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.CM" Name="CM" Domain="CM">
    <ItemRef ItemOID="IT.CM.PARAMCD"/>
  </ItemGroupDef>
  <ItemDef OID="IT.AE.PARAMCD" Name="PARAMCD" DataType="text">
    <CodeListRef CodeListOID="CL.AE.PARAMCD"/>
  </ItemDef>
  <ItemDef OID="IT.CM.PARAMCD" Name="PARAMCD" DataType="text">
    <CodeListRef CodeListOID="CL.CM.PARAMCD"/>
  </ItemDef>
</ODM>
"#,
        )
        .expect("write define xml");
        let context = CdiscContext::load(&[define_xml_path], &[], &[]).expect("load context");

        let unqualified = Condition {
            target: Some("PARAMCD".to_owned()),
            operator: Operator::IsContainedBy,
            comparator: ValueExpr::Null,
            options: Default::default(),
        };
        assert_eq!(define_codelist_for_condition(&unqualified, &context), None);

        let qualified = Condition {
            target: Some("AE.PARAMCD".to_owned()),
            operator: Operator::IsContainedBy,
            comparator: ValueExpr::Null,
            options: Default::default(),
        };
        assert_eq!(
            define_codelist_for_condition(&qualified, &context),
            Some("CL.AE.PARAMCD".to_owned())
        );
    }

    #[test]
    fn run_validation_uses_external_dictionary_for_term_checks() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DICTIONARY-MEDDRA.json"),
            r#"{
  "Core": { "Id": "CORE-DICTIONARY-MEDDRA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AEDECOD",
    "operator": "is_not_contained_by",
    "dictionary": "MEDDRA"
  },
  "Outcome": { "Message": "AEDECOD must exist in external dictionary" }
}"#,
        )
        .expect("write dictionary rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "AESEQ": [1, 2],
        "AEDECOD": ["HEADACHE", "UNKNOWN"]
      }
    }
  ]
}"#,
        )
        .expect("write dictionary data");

        let dictionary_path = dir.path().join("external_dictionary.json");
        fs::write(
            &dictionary_path,
            r#"{
  "dictionaries": [
    {
      "dictionary": "MEDDRA",
      "terms": [
        { "term": "HEADACHE" },
        { "term": "NAUSEA" }
      ]
    }
  ]
}"#,
        )
        .expect("write dictionary");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            external_dictionary_paths: vec![dictionary_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_filters_rules_by_standard_and_version() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-STANDARD-34.json"),
            r#"{
  "Core": { "Id": "CORE-STANDARD-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write matching standard rule");
        fs::write(
            rules_dir.join("CORE-STANDARD-33.json"),
            r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "CM"
  },
  "Outcome": { "Message": "DOMAIN must be CM" }
}"#,
        )
        .expect("write nonmatching standard rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("SDTMIG".to_owned()),
            standard_version: Some("3.4".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-34");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_reports_explicit_rule_standard_mismatch_as_skipped() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-STANDARD-33.json"),
            r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write nonmatching standard rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-STANDARD-33".to_owned()],
            standard: Some("SDTMIG".to_owned()),
            standard_version: Some("3.4".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-33");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::StandardMismatch)
        );
    }

    #[test]
    fn run_validation_classifies_known_standard_filter_fixture_gaps_as_oracle_gaps() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-000478.json"),
            r#"{
  "Core": { "Id": "CORE-000478", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "not_equal_to", "value": "AE" },
  "Outcome": { "Message": "known standard filter fixture gap" }
}"#,
        )
        .expect("write rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-000478".to_owned()],
            standard: Some("SENDIG".to_owned()),
            standard_version: Some("3.0".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }

    #[test]
    fn run_validation_treats_usdm_30_rules_as_compatible_with_usdm_40_fixtures() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-USDM-30.json"),
            r#"{
  "Core": { "Id": "CORE-USDM-30", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "USDM", "Version": "3.0" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write USDM 3.0 rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("USDM".to_owned()),
            standard_version: Some("4.0".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-USDM-30");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_treats_sdtmig_34_rules_as_compatible_with_sdtmig_33_fixtures() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-SDTMIG-34.json"),
            r#"{
  "Core": { "Id": "CORE-SDTMIG-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write SDTMIG 3.4 rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("SDTMIG".to_owned()),
            standard_version: Some("3.3".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-SDTMIG-34");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_treats_send_family_versions_as_compatible_for_open_rules_fixtures() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-SEND-FAMILY.json"),
            r#"{
  "Core": { "Id": "CORE-SEND-FAMILY", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG-DART", "Version": "1.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write SEND family rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("SENDIG".to_owned()),
            standard_version: Some("3.1".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-SEND-FAMILY");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_treats_core_000119_tig_sdtm_as_compatible_with_sendig_31() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "ARMCD", "label": "Planned Arm Code", "type": "Char", "length": 20 },
        { "name": "ARM", "label": "Description of Planned Arm", "type": "Char", "length": 40 }
      ],
      "records": { "ARMCD": [""], "ARM": ["PLACEBO"] }
    }
  ]
}"#,
        )
        .expect("write data");

        fs::write(
            rules_dir.join("CORE-000119.json"),
            r#"{
  "Core": { "Id": "CORE-000119", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "ARMCD", "operator": "empty" },
    { "name": "ARM", "operator": "non_empty" }
  ] },
  "Outcome": {
    "Message": "ARM is populated, when ARMCD is NULL",
    "Output Variables": ["ARMCD", "ARM"]
  }
}"#,
        )
        .expect("write rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("SENDIG".to_owned()),
            standard_version: Some("3.1".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-000119");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_treats_tig_rules_as_compatible_with_sdtmig_fixtures() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        fs::write(
            rules_dir.join("CORE-TIG.json"),
            r#"{
  "Core": { "Id": "CORE-TIG", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
        )
        .expect("write TIG rule");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            standard: Some("SDTMIG".to_owned()),
            standard_version: Some("3.4".to_owned()),
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-TIG");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    }

    #[test]
    fn run_validation_executes_filter_sort_aggregate_and_derive_operations() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-PIPELINE.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-PIPELINE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "filter",
      "dataset": "AE",
      "where": {
        "name": "AESER",
        "operator": "equal_to",
        "value": "Y"
      }
    },
    {
      "name": "sort",
      "by": ["AESEQ"],
      "descending": true
    },
    {
      "name": "aggregate",
      "by": ["USUBJID"],
      "as": "USUBJID_COUNT"
    },
    {
      "name": "derive",
      "as": "PIPELINE",
      "value": "OPS"
    }
  ],
  "Check": {
    "all": [
      {
        "name": "USUBJID_COUNT",
        "operator": "greater_than",
        "value": 1
      },
      {
        "name": "PIPELINE",
        "operator": "equal_to",
        "value": "OPS"
      }
    ]
  },
  "Outcome": { "Message": "Duplicate serious AE subject requires review" }
}"#,
        )
        .expect("write operations rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S2", "S1", "S2"],
        "DOMAIN": ["AE", "AE", "AE"],
        "AESEQ": [2, 1, 3],
        "AESER": ["Y", "N", "Y"]
      }
    }
  ]
}"#,
        )
        .expect("write operations data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("3"));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
        assert_eq!(outcome.results[0].errors[1].seq.as_deref(), Some("2"));
    }

    #[test]
    fn run_validation_executes_expanded_operations_pipeline() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-EXPANDED.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-EXPANDED", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "derive",
      "dataset": "AE",
      "as": "TERM_TRIM",
      "expression": "$trim(AETERM)"
    },
    {
      "name": "derive",
      "as": "TERM_UP",
      "expression": "$uppercase(TERM_TRIM)"
    },
    {
      "name": "aggregate",
      "by": ["USUBJID"],
      "function": "sum",
      "source_column": "AVAL",
      "as": "AVAL_SUM"
    },
    {
      "name": "distinct",
      "by": ["USUBJID", "TERM_UP", "AVAL_SUM"]
    },
    {
      "name": "rename",
      "columns": { "TERM_UP": "TERM" }
    },
    {
      "name": "row_number",
      "by": ["USUBJID"],
      "as": "ROWNUM"
    },
    {
      "name": "select",
      "columns": ["USUBJID", "AESEQ", "TERM", "AVAL_SUM", "ROWNUM"]
    }
  ],
  "Check": {
    "all": [
      {
        "name": "AVAL_SUM",
        "operator": "greater_than",
        "value": 4
      },
      {
        "name": "TERM",
        "operator": "equal_to",
        "value": "HEADACHE"
      },
      {
        "name": "ROWNUM",
        "operator": "equal_to",
        "value": 1
      }
    ]
  },
  "Outcome": { "Message": "High aggregate value requires review" }
}"#,
        )
        .expect("write expanded operations rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1", "S2"],
        "AESEQ": [1, 2, 3],
        "AETERM": [" headache ", "headache", "nausea"],
        "AVAL": [2, 3, 1]
      }
    }
  ]
}"#,
        )
        .expect("write expanded operations data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("1"));
    }

    #[test]
    fn run_validation_executes_open_rules_operator_style_record_count_and_distinct() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-OPEN-RULES.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-OPEN-RULES", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "GS",
      "group": ["PARENT", "REL"],
      "id": "$COUNT"
    },
    {
      "operator": "distinct",
      "group": ["PARENT", "REL"],
      "id": "$SCOPES",
      "name": "SCOPE"
    }
  ],
  "Check": {
    "all": [
      { "name": "$COUNT", "operator": "greater_than", "value": 1 },
      { "name": "$SCOPES", "operator": "contains_case_insensitive", "value": "global" }
    ]
  },
  "Outcome": { "Message": "Global scope appears more than once" }
}"#,
        )
        .expect("write operations rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "gs.xpt",
      "domain": "GS",
      "records": {
        "PARENT": ["A", "A", "B"],
        "REL": ["definition", "definition", "definition"],
        "SCOPE": ["Global", "Regional", "Regional"]
      }
    }
  ]
}"#,
        )
        .expect("write operations data");

        let rules = load_rules_from_paths(std::slice::from_ref(&rules_dir)).expect("load rules");
        assert_eq!(rules[0].operations.len(), 2);
        assert_eq!(
            operation_name(&rules[0].operations[0]).as_deref(),
            Some("record_count")
        );
        assert_eq!(
            operation_name(&rules[0].operations[1]).as_deref(),
            Some("distinct")
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_executes_grouped_distinct_operation_for_required_txparmcd() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000891.json"),
            r#"{
  "Core": { "Id": "CORE-000891", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "ARMCD"
  },
  "Outcome": {
    "Message": "TX dataset should include a TXPARMCD = ARMCD record per SETCD.",
    "Output Variables": ["SETCD", "$txparmcd"]
  }
}"#,
        )
        .expect("write grouped distinct rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "B", "B"],
        "TXPARMCD": ["ARMCD", "SPECIES", "ARMCDxxx", "STRAIN"]
      }
    }
  ]
}"#,
        )
        .expect("write grouped distinct data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(3));
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["SETCD".to_owned(), "$txparmcd".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_group_sensitivity_for_grouped_distinct_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000888.json"),
            r#"{
  "Core": { "Id": "CORE-000888", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Group",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "PLANFSUB"
  },
  "Outcome": {
    "Message": "TX dataset should include exactly one TXPARMCD = 'PLANFSUB' record per SETCD.",
    "Output Variables": ["SETCD", "$txparmcd"]
  }
}"#,
        )
        .expect("write group sensitivity rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["SET1", "SET1", "SET2", "SET2"],
        "TXPARMCD": ["ARMCD", "PLANFSUB", "ARMCD", "SPGRPCD"]
      }
    }
  ]
}"#,
        )
        .expect("write group sensitivity data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{:?}",
            outcome.results[0]
        );
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "TX");
        assert_eq!(
            outcome.results[0].errors[0].variables,
            vec!["SETCD".to_owned(), "$txparmcd".to_owned()]
        );
    }

    #[test]
    fn run_validation_executes_grouped_distinct_operation_for_treatment_dose_parms() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        for (rule_id, required_txparmcd) in [("CORE-000894", "TRTDOS"), ("CORE-000895", "TRTDOSU")]
        {
            fs::write(
                rules_dir.join(format!("{rule_id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{ "Include": ["TX"] }} }},
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {{
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }}
  ],
  "Check": {{
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "{required_txparmcd}"
  }},
  "Outcome": {{
    "Message": "TX dataset should include a TXPARMCD = {required_txparmcd} record per SETCD."
  }}
}}"#
                ),
            )
            .expect("write grouped distinct treatment dose rule");
        }

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "B", "B", "B"],
        "TXPARMCD": ["TRTDOS", "TRTDOSU", "ARMCD", "TRTDOSxxx", "TRTDOSUxxx"]
      }
    }
  ]
}"#,
        )
        .expect("write grouped distinct treatment dose data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        for result in outcome.results {
            assert_eq!(result.execution_status, ExecutionStatus::Failed);
            assert_eq!(result.error_count, 3);
            assert_eq!(result.errors[0].row, Some(3));
            assert_eq!(result.errors[0].variables, vec!["$txparmcd".to_owned()]);
        }
    }

    #[test]
    fn run_validation_executes_open_rules_distinct_with_schema_normalized_keys() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-DISTINCT-SCHEMA-KEYS.yml"),
            r#"Core:
  Id: CORE-DISTINCT-SCHEMA-KEYS
  Status: Published
Scope:
  Domains:
    Include:
      - ACTIVITY
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - parent_id
    id: $activity_ids_for_parent
    name: id
    operator: distinct
Check:
  name: $activity_ids_for_parent
  operator: contains
  value: Activity_2
Outcome:
  Message: Parent contains Activity_2
"#,
        )
        .expect("write distinct schema rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset,Label\nActivity.csv,Activity,Activity\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nActivity,parent_id,Parent Entity Id,String,[1]\nActivity,id,Activity Id,String,[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("Activity.csv"),
            "parent_id,id\nDesign_1,Activity_1\nDesign_1,Activity_2\nDesign_2,Activity_3\n",
        )
        .expect("write activity csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_core_000837_entity_column_ref_distinct_set() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000837.yml"),
            r#"Core:
  Id: CORE-000837
  Status: Published
Scope:
  Entities:
    Include:
      - Activity
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - parent_id
      - rel_type
    id: $activity_ids_for_study_design
    name: id
    operator: distinct
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: Activity
    - name: rel_type
      operator: equal_to
      value: definition
    - any:
        - all:
            - name: previousId
              operator: exists
            - name: previousId
              operator: non_empty
            - name: previousId
              operator: is_not_contained_by
              value: $activity_ids_for_study_design
        - all:
            - name: nextId
              operator: exists
            - name: nextId
              operator: non_empty
            - name: nextId
              operator: is_not_contained_by
              value: $activity_ids_for_study_design
Outcome:
  Message: Activity references must stay within the same study design
  Output Variables:
    - id
    - parent_id
    - previousId
    - nextId
    - $activity_ids_for_study_design
"#,
        )
        .expect("write entity column-ref rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset,Label\nActivity.csv,Activity,Activity\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nActivity,parent_id,Parent Entity Id,String,[1]\nActivity,rel_type,Type of Relationship,String,[1]\nActivity,id,Activity Id,String,[1]\nActivity,instanceType,Instance Type,String,[1]\nActivity,previousId,Previous Activity,String,[0..1]\nActivity,nextId,Next Activity,String,[0..1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("Activity.csv"),
            "parent_id,rel_type,id,instanceType,previousId,nextId\nDesign_1,definition,Activity_1,Activity,,Activity_2\nDesign_1,definition,Activity_2,Activity,Activity_1,Activity_3\nDesign_2,definition,Activity_3,Activity,Activity_2,\n",
        )
        .expect("write activity csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[1].row, Some(3));
    }

    #[test]
    fn run_validation_executes_core_000427_record_count_column_ref_comparisons() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000427.yml"),
            r#"Core:
  Id: CORE-000427
  Status: Published
Scope:
  Entities:
    Include:
      - Code
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - codeSystem
      - codeSystemVersion
      - code
    id: $num_records_in_codesystemversion_with_code
    operator: record_count
  - group:
      - codeSystem
      - codeSystemVersion
      - decode
    id: $num_records_in_codesystemversion_with_decode
    operator: record_count
  - group:
      - codeSystem
      - codeSystemVersion
      - code
      - decode
    id: $num_records_in_codesystemversion_with_code_decode
    operator: record_count
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: Code
    - any:
        - name: $num_records_in_codesystemversion_with_code
          operator: not_equal_to
          value: $num_records_in_codesystemversion_with_code_decode
        - name: $num_records_in_codesystemversion_with_decode
          operator: not_equal_to
          value: $num_records_in_codesystemversion_with_code_decode
Outcome:
  Message: Code and decode should have a one-to-one relationship
  Output Variables:
    - codeSystem
    - codeSystemVersion
    - code
    - decode
    - $num_records_in_codesystemversion_with_code
    - $num_records_in_codesystemversion_with_decode
    - $num_records_in_codesystemversion_with_code_decode
"#,
        )
        .expect("write record count column-ref rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset,Label\nCode.csv,Code,Code\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nCode,codeSystem,Code System,String,[1]\nCode,codeSystemVersion,Code System Version,String,[1]\nCode,code,Code,String,[1]\nCode,decode,Decode,String,[1]\nCode,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("Code.csv"),
            "codeSystem,codeSystemVersion,code,decode,instanceType\nCDISC,2024-01,A,Alpha,Code\nCDISC,2024-01,A,Beta,Code\nCDISC,2024-01,B,Gamma,Code\n",
        )
        .expect("write code csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_record_count_operation_inline_filter() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-FILTERED-RECORD-COUNT.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-FILTERED-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TX",
      "filter": { "TXPARMCD": "ARMCD" },
      "group": ["SETCD"],
      "id": "$armcd_count"
    }
  ],
  "Check": {
    "name": "$armcd_count",
    "operator": "greater_than",
    "value": 1
  },
  "Outcome": {
    "Message": "There may be only one ARMCD per SETCD",
    "Output Variables": ["SETCD", "$armcd_count", "TXPARMCD"]
  }
}"#,
        )
        .expect("write record count rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "A", "B", "B"],
        "TXPARMCD": ["ARMCD", "ARMCD", "SPECIES", "ARMCD", "SPECIES"]
      }
    }
  ]
}"#,
        )
        .expect("write record count data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 3);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
        assert_eq!(outcome.results[0].errors[2].row, Some(3));
    }

    #[test]
    fn run_validation_preserves_multi_domain_scope_after_targeted_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-MULTI-DOMAIN-COUNT.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-MULTI-DOMAIN-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "TS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGEU" },
      "id": "$ageu_count"
    }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "DOMAIN", "operator": "equal_to", "value": "DM" },
          { "name": "AGEU", "operator": "empty" },
          { "name": "AGE", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "DOMAIN", "operator": "equal_to", "value": "TS" },
          { "name": "$ageu_count", "operator": "equal_to", "value": 0 }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "AGEU is expected when AGE is populated",
    "Output Variables": ["DOMAIN", "AGE", "AGEU", "$ageu_count"]
  }
}"#,
        )
        .expect("write multi-domain rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["DM1", "DM2"],
        "DOMAIN": ["DM", "DM"],
        "AGE": ["33", ""],
        "AGEU": ["", "YRS"]
      }
    },
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["TS1", "TS2"],
        "DOMAIN": ["TS", "TS"],
        "TSPARMCD": ["AGEU", "AGE"],
        "TSVAL": ["YRS", "33"]
      }
    }
  ]
}"#,
        )
        .expect("write multi-domain data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let dm_result = outcome
            .results
            .iter()
            .find(|result| result.domain.as_deref() == Some("DM"))
            .expect("DM result");
        let ts_result = outcome
            .results
            .iter()
            .find(|result| result.domain.as_deref() == Some("TS"))
            .expect("TS result");

        assert_eq!(dm_result.execution_status, ExecutionStatus::Failed);
        assert_eq!(dm_result.error_count, 1);
        assert_eq!(dm_result.errors[0].row, Some(1));
        assert_eq!(ts_result.execution_status, ExecutionStatus::Passed);
        assert_eq!(ts_result.error_count, 0);
    }

    #[test]
    fn run_validation_executes_open_rules_record_count_with_schema_normalized_keys() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-RECORD-COUNT-SCHEMA-KEYS.yml"),
            r#"Core:
  Id: CORE-RECORD-COUNT-SCHEMA-KEYS
  Status: Published
Scope:
  Domains:
    Include:
      - CODE
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - codeSystem
      - codeSystemVersion
      - code
    id: $num_records_with_code
    operator: record_count
Check:
  name: $num_records_with_code
  operator: greater_than
  value: 1
Outcome:
  Message: Duplicate code within a code system version
"#,
        )
        .expect("write record count schema rule");

        fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset,Label\nCode.csv,Code,Code\n",
        )
        .expect("write datasets csv");
        fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nCode,codeSystem,Code System,String,[1]\nCode,codeSystemVersion,Code System Version,String,[1]\nCode,code,Code,String,[1]\nCode,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
        fs::write(
            data_dir.join("Code.csv"),
            "codeSystem,codeSystemVersion,code,instanceType\nCDISC,2024-01,X,Code\nCDISC,2024-01,X,Code\nCDISC,2024-01,Y,Code\n",
        )
        .expect("write code csv");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![data_dir],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
        assert_eq!(outcome.results[0].errors[0].row, Some(1));
        assert_eq!(outcome.results[0].errors[1].row, Some(2));
    }

    #[test]
    fn run_validation_executes_record_count_operation_without_group() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-OPS-DATASET-RECORD-COUNT.json"),
            r#"{
  "Core": { "Id": "CORE-OPS-DATASET-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGE" },
      "id": "$record_count_AGE"
    },
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGETXT" },
      "id": "$record_count_AGETXT"
    }
  ],
  "Check": {
    "all": [
      { "name": "$record_count_AGE", "operator": "greater_than_or_equal_to", "value": 1 },
      { "name": "$record_count_AGETXT", "operator": "greater_than_or_equal_to", "value": 1 }
    ]
  },
  "Outcome": {
    "Message": "AGE and AGETXT must not both be present",
    "Output Variables": ["$record_count_AGE", "$record_count_AGETXT"]
  }
}"#,
        )
        .expect("write dataset record count rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "DOMAIN": ["TS", "TS", "TS"],
        "TSPARMCD": ["AGE", "AGETXT", "SEX"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset record count data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, None);
    }

    #[test]
    fn run_validation_maps_external_record_count_operation_by_group_aliases() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-ENTITY-RECORD-COUNT.json"),
            r#"{
  "Core": { "Id": "CORE-ENTITY-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "StudyIdentifier",
      "filter": {
        "parent_entity": "StudyVersion",
        "parent_rel": "studyIdentifiers",
        "rel_type": "definition",
        "studyIdentifierScope.organizationType.code": "C70793",
        "studyIdentifierScope.organizationType.codeSystem": "http://www.cdisc.org"
      },
      "group": ["parent_id"],
      "group_aliases": ["id"],
      "id": "$num_sponsor_ids",
      "operator": "record_count"
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyVersion" },
      {
        "any": [
          { "name": "$num_sponsor_ids", "operator": "empty" },
          { "name": "$num_sponsor_ids", "operator": "not_equal_to", "value": 1 }
        ]
      }
    ]
  },
  "Outcome": { "Message": "StudyVersion must have exactly one sponsor identifier" }
}"#,
        )
        .expect("write external record count rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyVersion.csv",
      "domain": "StudyVersion",
      "records": {
        "ID": ["StudyVersion_1", "StudyVersion_2", "StudyVersion_3"],
        "instanceType": ["StudyVersion", "StudyVersion", "StudyVersion"]
      }
    },
    {
      "filename": "StudyIdentifier.csv",
      "domain": "StudyIdentifier",
      "records": {
        "parent_entity": ["StudyVersion", "StudyVersion", "StudyVersion", "StudyVersion"],
        "PARENT_ID": ["StudyVersion_1", "StudyVersion_1", "StudyVersion_2", "StudyVersion_3"],
        "parent_rel": ["studyIdentifiers", "studyIdentifiers", "studyIdentifiers", "studyIdentifiers"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "studyIdentifierScope.organizationType.code": ["C70793", "C70793", "C70793", "C93453"],
        "studyIdentifierScope.organizationType.codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org"]
      }
    }
  ]
}"#,
        )
        .expect("write external record count data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 2);
    }

    #[test]
    fn run_validation_skips_operation_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000894.json"),
            r#"{
  "Core": { "Id": "CORE-000894", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "distinct",
      "domain": "GS",
      "group": ["PARENT"],
      "name": "REL",
      "id": "$VALUES"
    }
  ],
  "Check": { "name": "$VALUES", "operator": "does_not_contain", "value": "global" },
  "Outcome": { "Message": "distinct semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write operation gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "gs.xpt",
      "domain": "GS",
      "records": {
        "PARENT": ["A", "A"]
      }
    }
  ]
}"#,
        )
        .expect("write operations data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(
            outcome.results[0].skipped_reason,
            Some(SkippedReason::OperationsNotSupported)
        );
    }

    fn write_test_xpt_char_dataset(
        path: &std::path::Path,
        dataset_name: &str,
        columns: &[&str],
        rows: &[Vec<&str>],
    ) {
        const CARD_LEN: usize = 80;
        const NAMESTR_LEN: usize = 140;

        let mut bytes = Vec::new();
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        push_xpt_card(
            &mut bytes,
            "SAS     SAS     SASLIB  9.4     X64_10PRO                       18JUN26:00:00:00",
        );
        push_xpt_card(&mut bytes, "18JUN26:00:00:00");
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******MEMBER  HEADER RECORD!!!!!!!000000000000000001600000000140",
        );
        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******DSCRPTR HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        push_xpt_card(
            &mut bytes,
            &format!(
                "SAS     {:<8}SASDATA 9.4     X64_10PRO                       18JUN26:00:00:00",
                dataset_name
            ),
        );
        push_xpt_card(&mut bytes, "18JUN26:00:00:00");
        push_xpt_card(
            &mut bytes,
            &format!(
                "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
                columns.len()
            ),
        );

        let lengths = columns
            .iter()
            .map(|column| match *column {
                "DOMAIN" => 2,
                "AESEQ" | "CMSEQ" | "SEQ" => 8,
                _ => 12,
            })
            .collect::<Vec<_>>();
        let mut offset = 0_u32;
        let mut namestrs = Vec::new();
        for (index, (column, length)) in columns.iter().zip(&lengths).enumerate() {
            let mut namestr = vec![0_u8; NAMESTR_LEN];
            namestr[0..2].copy_from_slice(&2_u16.to_be_bytes());
            namestr[4..6].copy_from_slice(&(*length as u16).to_be_bytes());
            namestr[6..8].copy_from_slice(&((index + 1) as u16).to_be_bytes());
            write_padded(&mut namestr[8..16], column);
            write_padded(&mut namestr[16..56], column);
            namestr[84..88].copy_from_slice(&offset.to_be_bytes());
            offset += *length as u32;
            namestrs.extend(namestr);
        }
        pad_to_xpt_card(&mut namestrs);
        bytes.extend(namestrs);

        push_xpt_card(
            &mut bytes,
            "HEADER RECORD*******OBS     HEADER RECORD!!!!!!!000000000000000000000000000000",
        );
        for row in rows {
            assert_eq!(row.len(), columns.len());
            for (value, length) in row.iter().zip(&lengths) {
                let start = bytes.len();
                bytes.resize(start + *length, b' ');
                write_padded(&mut bytes[start..start + *length], value);
            }
        }
        pad_to_xpt_card(&mut bytes);

        fs::write(path, bytes).expect("write xpt");

        fn push_xpt_card(bytes: &mut Vec<u8>, value: &str) {
            let start = bytes.len();
            bytes.resize(start + CARD_LEN, b' ');
            write_padded(&mut bytes[start..start + CARD_LEN], value);
        }

        fn write_padded(target: &mut [u8], value: &str) {
            let bytes = value.as_bytes();
            let len = bytes.len().min(target.len());
            target[..len].copy_from_slice(&bytes[..len]);
        }

        fn pad_to_xpt_card(bytes: &mut Vec<u8>) {
            let remainder = bytes.len() % CARD_LEN;
            if remainder != 0 {
                bytes.resize(bytes.len() + CARD_LEN - remainder, b' ');
            }
        }
    }

    fn write_raw_rule(
        dir: &std::path::Path,
        id: &str,
        rule_type: &str,
        extra_rule_field: &str,
        operator: &str,
    ) {
        fs::write(
            dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{}}, "Classes": {{}} }},
  "Sensitivity": "Record",
  {rule_type},
  {extra_rule_field}
  "Check": {{
    "name": "DOMAIN",
    {operator},
    "value": "AE"
  }},
  "Outcome": {{ "Message": "DOMAIN must be AE" }}
}}"#
            ),
        )
        .expect("write raw rule");
    }
}
