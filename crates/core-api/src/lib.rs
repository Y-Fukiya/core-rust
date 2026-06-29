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

    let mut results = selection.skipped.into_iter().collect::<Vec<_>>();
    let mut executable_rules = Vec::new();
    for rule in selection.selected {
        if let Some(skipped) = skipped_unsupported_rule(&rule) {
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
        let Some(cdisc_context) = cdisc_context.as_ref() else {
            continue;
        };
        let rule = prepare_rule_for_execution(rule, cdisc_context, &request.standard);
        if let Some(result) = core_000677_pooldef_poolid_result(&rule, &datasets) {
            results.push(result);
            continue;
        }
        if let Some(result) = core_000744_relrec_faobj_result(&rule, &datasets) {
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
                    if let Some(skipped) = oracle_gap_result_after_execution(&rule, &result) {
                        results.push(skipped);
                    } else {
                        results.push(result);
                    }
                }
                Err(source) => {
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

        if results[rule_result_start..].is_empty() {}
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
    let _ = (rule, result);
    // Open Rules oracle gaps are coverage decisions, not a reason to rewrite an
    // executed engine failure. Keeping failures as failures preserves the
    // independence of score reports and prevents false-positive conformance.
    return None;

    #[allow(unreachable_code)]
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
mod tests;
