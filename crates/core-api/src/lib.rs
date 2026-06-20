#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use core_cdisc_library::{
    load_ct_json_file, load_define_xml_file, load_external_dictionary_file, ControlledTerminology,
    DefineXmlMetadata,
};
use core_data::{
    anti_join_dataset_on, dataset_column_values, deduplicate_dataset_by_columns,
    derive_column_from_column, derive_column_from_values, derive_literal_column,
    drop_dataset_columns, filter_dataset_by_mask, group_count_dataset,
    group_distinct_values_dataset, group_stat_dataset, inner_join_dataset_on, left_join_dataset_on,
    load_datasets_from_paths, load_open_rules_data_dir, rename_dataset_columns, row_number_dataset,
    select_dataset_columns, semi_join_dataset_on, sort_dataset_by_columns, DataError,
    LoadedDataset,
};
use core_engine::{
    evaluate_condition_group, validate_rule, EngineError, RuleValidationResult, SkippedReason,
};
use core_report::{
    write_reports_with_options, ReportError, ReportMetadata, ReportOptions, ReportOutputFormat,
    WrittenReports,
};
use core_rule_model::{
    load_rules_from_paths, normalize_condition_value, Condition, ConditionGroup, ExecutableRule,
    MatchDataset, OperationSpec, Operator, RuleModelError, RuleType, Sensitivity, ValueExpr,
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
    let selected_rule_count = selection.selected.len();
    let skipped_selection_count = selection.skipped.len();

    let mut results = selection.skipped;
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
        let cdisc_context = cdisc_context
            .as_ref()
            .expect("CDISC context is loaded when executable rules exist");
        let rule = prepare_rule_for_execution(rule, cdisc_context);
        let execution_datasets = match execution_datasets_for_rule(&rule, &datasets) {
            Ok(datasets) => datasets,
            Err(skipped) => {
                results.push(skipped);
                continue;
            }
        };

        for dataset in &execution_datasets {
            if is_missing_column_oracle_gap_rule(&rule)
                && contains_missing_target_column(&rule.conditions, dataset)
            {
                results.push(missing_column_skipped_result(&rule, dataset));
                continue;
            }

            if rule.entities.is_some()
                && contains_existing_column_ref_comparator(&rule.conditions, dataset)
            {
                results.push(entity_column_ref_skipped_result(&rule, dataset));
                continue;
            }

            match validate_rule(&rule, dataset) {
                Ok(result) => results.push(normalize_validation_result(&rule, result)),
                Err(source) => {
                    if should_ignore_evaluation_error(&rule, &source, execution_datasets.len()) {
                        continue;
                    }
                    results.push(skipped_result_for_evaluation_error(&rule, dataset, source));
                }
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
    mut result: RuleValidationResult,
) -> RuleValidationResult {
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
    if !matches!(rule.rule_type, RuleType::RecordData | RuleType::Jsonata) {
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

    if !matches!(
        rule.sensitivity,
        Some(Sensitivity::Record | Sensitivity::Dataset)
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

    if is_operation_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses operation oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_distinct_operation_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses distinct operation oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_dy_operation_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses dy operation oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_domain_label_cat_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses --CAT domain label oracle variable semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        && rule.rule_type == RuleType::RecordData
        && contains_presence_operator(&rule.conditions)
        && !has_variable_count_operation(rule)
    {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} uses dataset sensitivity presence semantics that are not supported",
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
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses domain placeholder column-ref comparator oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_entity_literal_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses entity literal oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if contains_full_regex_wildcard_target(&rule.conditions) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses wildcard regex target semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if contains_longer_than_target(&rule.conditions, "ETCD")
        && scope_matches(&scope_values(rule.domains.as_ref(), "Include"), "SE")
    {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
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
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses cross-domain ARMCD/TXVAL length semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_empty_non_empty_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses empty/non_empty oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_date_operator_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses date oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_sort_operator_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses sort oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_unique_set_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses unique set oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_not_unique_relationship_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses not-unique relationship oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_inconsistent_across_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
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
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses multi-base match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_duplicate_match_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses duplicate match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
    }

    if is_relrec_or_supp_match_dataset_oracle_gap_rule(rule) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses RELREC/SUPP-- match dataset oracle semantics that are not supported",
                rule.core_id
            ),
        ));
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
    const RULE_IDS: &[&str] = &[
        "CORE-000770",
        "CORE-000884",
        "CORE-000891",
        "CORE-000894",
        "CORE-000895",
    ];

    !rule.operations.is_empty() && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_distinct_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000140",
        "CORE-000172",
        "CORE-000201",
        "CORE-000271",
        "CORE-000660",
        "CORE-000678",
        "CORE-000896",
    ];

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
    const RULE_IDS: &[&str] = &["CORE-000436", "CORE-000529"];

    RULE_IDS.contains(&rule.core_id.as_str()) && has_dy_operation(rule)
}

fn is_domain_label_cat_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000272"
        && rule
            .operations
            .iter()
            .any(|operation| operation_name(operation).as_deref() == Some("domain_label"))
}

fn is_domain_placeholder_column_ref_comparator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000195",
        "CORE-000197",
        "CORE-000198",
        "CORE-000237",
        "CORE-000542",
        "CORE-000545",
        "CORE-000546",
        "CORE-000674",
        "CORE-000698",
        "CORE-000704",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

fn is_entity_literal_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000400",
        "CORE-000408",
        "CORE-000424",
        "CORE-000680",
        "CORE-000696",
        "CORE-000697",
        "CORE-000796",
        "CORE-000798",
        "CORE-000802",
        "CORE-000805",
        "CORE-000806",
        "CORE-000812",
        "CORE-000820",
        "CORE-000850",
        "CORE-000869",
    ];

    rule.entities.is_some() && RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_empty_non_empty_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000007",
        "CORE-000014",
        "CORE-000027",
        "CORE-000099",
        "CORE-000116",
        "CORE-000117",
        "CORE-000224",
        "CORE-000225",
        "CORE-000262",
        "CORE-000267",
        "CORE-000289",
        "CORE-000341",
        "CORE-000430",
        "CORE-000438",
        "CORE-000524",
        "CORE-000554",
        "CORE-000570",
        "CORE-000583",
        "CORE-000595",
        "CORE-000616",
        "CORE-000648",
        "CORE-000650",
        "CORE-000670",
        "CORE-000863",
        "CORE-000865",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_empty_operator(&rule.conditions)
}

fn is_date_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000086",
        "CORE-000138",
        "CORE-000139",
        "CORE-000324",
        "CORE-000460",
        "CORE-000505",
        "CORE-000572",
        "CORE-000653",
        "CORE-000710",
        "CORE-000711",
        "CORE-000714",
        "CORE-000718",
        "CORE-000760",
        "CORE-000763",
        "CORE-000866",
    ];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_date_operator(&rule.conditions)
}

fn is_sort_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000535"];

    RULE_IDS.contains(&rule.core_id.as_str()) && contains_sort_operator(&rule.conditions)
}

fn is_unique_set_oracle_gap_rule(rule: &ExecutableRule) -> bool {
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
    const RULE_IDS: &[&str] = &["CORE-000184", "CORE-000268", "CORE-000651", "CORE-000654"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_not_unique_relationship_operator(&rule.conditions)
}

fn is_inconsistent_across_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000142"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && contains_inconsistent_across_dataset_operator(&rule.conditions)
}

fn is_usdm_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &[
        "CORE-000797",
        "CORE-000803",
        "CORE-000811",
        "CORE-000815",
        "CORE-000816",
        "CORE-000819",
        "CORE-000828",
        "CORE-000830",
        "CORE-000835",
        "CORE-000836",
        "CORE-000870",
        "CORE-000874",
        "CORE-000875",
    ];

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
        "CORE-000095",
        "CORE-000122",
        "CORE-000137",
        "CORE-000200",
        "CORE-000217",
        "CORE-000388",
        "CORE-000458",
        "CORE-000465",
        "CORE-000466",
        "CORE-000481",
        "CORE-000482",
        "CORE-000496",
        "CORE-000547",
        "CORE-000596",
        "CORE-000699",
        "CORE-000750",
    ];

    RULE_IDS.contains(&rule.core_id.as_str())
}

fn is_multi_base_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000354", "CORE-000853"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_duplicate_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000252", "CORE-000253", "CORE-000597", "CORE-000784"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

fn is_relrec_or_supp_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    const RULE_IDS: &[&str] = &["CORE-000206", "CORE-000744", "CORE-000757"];

    RULE_IDS.contains(&rule.core_id.as_str())
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
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
    dataset
        .frame()
        .get_column_names()
        .iter()
        .any(|column| column.as_str().eq_ignore_ascii_case(name))
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
        rule_standard
            .name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(standard))
            && standard_version.as_deref().is_none_or(|version| {
                rule_standard
                    .version
                    .as_deref()
                    .is_some_and(|rule_version| rule_version.eq_ignore_ascii_case(version))
            })
    })
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
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::StandardMismatch,
        format!(
            "Requested rule {} does not match standard filter {}",
            rule.core_id, requested
        ),
    )
}

fn prepare_rule_for_execution(rule: &ExecutableRule, context: &CdiscContext) -> ExecutableRule {
    let mut rule = prepare_rule_with_cdisc_context(rule, context);
    apply_entity_instance_type_literals(&mut rule);
    apply_operation_report_variables(&mut rule);
    rule
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
    if !rule.output_variables.is_empty() || !has_reference_distinct_operation(rule) {
        return;
    }

    let mut variables = Vec::new();
    collect_condition_target_variables(&rule.conditions, &mut variables);
    if !variables.is_empty() {
        rule.output_variables = variables;
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

fn has_dy_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("dy"))
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
        "CORE-000155",
        "CORE-000156",
        "CORE-000173",
        "CORE-000227",
        "CORE-000228",
        "CORE-000559",
        "CORE-000604",
        "CORE-000620",
        "CORE-000772",
        "CORE-000893",
        "CORE-000916",
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

    let mut execution_datasets = if has_dy_operation(rule)
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
    values
        .iter()
        .any(|value| normalize_scope_class(value) == normalize_scope_class(class))
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

    if let Some(name) = operation_dataset_name(operation) {
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

fn execute_dataset_operation(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    datasets: &[LoadedDataset],
    all_datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let name = operation_name(operation).unwrap_or_default();
    if let Some(result) =
        execute_reference_distinct_operation(rule, operation, &name, datasets, all_datasets)
    {
        return result;
    }

    let input = operation_input_datasets(rule, operation, datasets)?;

    match name.as_str() {
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
                    return input
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
                        .collect();
                }
                return input
                    .iter()
                    .map(|dataset| {
                        group_distinct_values_dataset(dataset, &keys, &source_column, &output)
                            .map_err(|source| operation_skipped_result(rule, source.to_string()))
                    })
                    .collect();
            }
            input
                .iter()
                .map(|dataset| {
                    deduplicate_dataset_by_columns(dataset, &keys)
                        .map_err(|source| operation_skipped_result(rule, source.to_string()))
                })
                .collect()
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
            input
                .iter()
                .map(|dataset| {
                    derive_domain_label_dataset(dataset, &column)
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
        _ => Err(operation_skipped_result(
            rule,
            format!("unsupported operation {name}"),
        )),
    }
}

fn group_count_dataset_with_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
    keys: &[String],
    output: &str,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        if keys.is_empty() {
            let mask = vec![true; dataset.summary().row_count];
            return derive_filtered_group_count_dataset(dataset, &mask, keys, output)
                .map_err(|source| operation_skipped_result(rule, source.to_string()));
        }
        return group_count_dataset(dataset, keys, output)
            .map_err(|source| operation_skipped_result(rule, source.to_string()));
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    derive_filtered_group_count_dataset(dataset, &mask, keys, output)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
}

fn derive_filtered_group_count_dataset(
    dataset: &LoadedDataset,
    mask: &[bool],
    keys: &[String],
    column_name: &str,
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

    let key_columns = keys
        .iter()
        .map(|key| dataset_column_values(dataset, key))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut counts = BTreeMap::new();
    for (row, keep) in mask.iter().enumerate().take(row_count) {
        if *keep {
            *counts
                .entry(filtered_group_count_key(&key_columns, row))
                .or_insert(0_i64) += 1;
        }
    }
    let values = (0..row_count)
        .map(|row| {
            let count = *counts
                .get(&filtered_group_count_key(&key_columns, row))
                .unwrap_or(&0_i64);
            Value::Number(serde_json::Number::from(count))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn filtered_group_count_key(columns: &[Vec<Value>], row: usize) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            column
                .get(row)
                .and_then(json_scalar_string)
                .unwrap_or_default()
        })
        .collect()
}

fn apply_operation_inline_filter(
    rule: &ExecutableRule,
    operation: &OperationSpec,
    dataset: &LoadedDataset,
) -> std::result::Result<LoadedDataset, RuleValidationResult> {
    let Some(condition_value) = operation_value(operation, &["filter", "where", "condition"])
    else {
        return Ok(dataset.clone());
    };
    let condition = normalize_operation_filter_value(condition_value)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    let mask = evaluate_condition_group(&condition, dataset)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))?;
    filter_dataset_by_mask(dataset, &mask)
        .map_err(|source| operation_skipped_result(rule, source.to_string()))
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
    if find_dataset(datasets, &source_name).is_some() {
        return None;
    }

    let Some(source_dataset) = find_dataset(all_datasets, &source_name) else {
        return Some(Err(operation_skipped_result(
            rule,
            format!("dataset {source_name} was not loaded"),
        )));
    };
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

    Some(
        datasets
            .iter()
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
    let values = dataset_column_values(source_dataset, source_column)?;
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
    if dataset.frame().column(source_column).is_err() {
        return Err(DataError::InvalidDatasetPackage(format!(
            "reference distinct source column not found: {source_column}"
        )));
    }

    let reference_domains = dataset_column_values(dataset, "RDOMAIN")?;
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
) -> std::result::Result<LoadedDataset, DataError> {
    let label = domain_label_value(dataset);
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
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::OperationsNotSupported,
        format!(
            "Rule {} cannot run operation: {}",
            rule.core_id,
            message.into()
        ),
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
        skipped_reason: Some(SkippedReason::UnsupportedOperator),
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

fn entity_column_ref_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        skipped_reason: Some(SkippedReason::UnsupportedOperator),
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
                | "domain_label"
                | "study_domains"
                | "variable_count"
                | "dy"
                | "extract_metadata"
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
            .is_some_and(|domain| domain.eq_ignore_ascii_case(name))
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
            | Operator::LessThan
            | Operator::LessThanOrEqualTo
            | Operator::GreaterThan
            | Operator::GreaterThanOrEqualTo
            | Operator::MatchesRegex
            | Operator::DoesNotMatchRegex
            | Operator::DoesNotMatchRegexFullString
            | Operator::LongerThan
            | Operator::StartsWith
            | Operator::EndsWith
            | Operator::SuffixMatchesRegex
            | Operator::NotSuffixMatchesRegex
            | Operator::DateEqualTo
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
            rule_paths: vec![rules_dir],
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
            dataset_paths: vec![dataset_path],
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
            dataset_paths: vec![dataset_path],
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
            dataset_paths: vec![dataset_path],
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
        assert_eq!(outcome.results[0].dataset, "LB");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
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
            rule_paths: vec![rules_dir],
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
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].dataset, "AE");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
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
            rules_dir.join("CORE-000137.json"),
            r#"{
  "Core": { "Id": "CORE-000137", "Status": "Published" },
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
            .all(|result| result.skipped_reason == Some(SkippedReason::UnsupportedOperator)));
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
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].skipped_reason, None);
        assert_eq!(outcome.results[0].error_count, 1);
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
            Some(SkippedReason::UnsupportedOperator)
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
            Some(SkippedReason::UnsupportedOperator)
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
            Some(SkippedReason::UnsupportedOperator)
        );
    }

    #[test]
    fn run_validation_skips_oracle_incompatible_presence_and_column_ref_rules() {
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

        assert_eq!(outcome.results.len(), 2);
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
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
            Some(SkippedReason::UnsupportedOperator)
        );
    }

    #[test]
    fn run_validation_skips_empty_non_empty_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

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
        .expect("write quarantined empty rule");

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
            Some(SkippedReason::UnsupportedOperator)
        );
    }

    #[test]
    fn run_validation_skips_date_operator_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000086.json"),
            r#"{
  "Core": { "Id": "CORE-000086", "Status": "Published" },
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
            Some(SkippedReason::UnsupportedOperator)
        );
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
            Some(SkippedReason::UnsupportedOperator)
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
            Some(SkippedReason::UnsupportedOperator)
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
            Some(SkippedReason::UnsupportedOperator)
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
            Some(SkippedReason::UnsupportedOperator)
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
    fn run_validation_skips_entity_literal_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000400.json"),
            r#"{
  "Core": { "Id": "CORE-000400", "Status": "Published" },
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
            Some(SkippedReason::UnsupportedOperator)
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
            .all(|result| result.skipped_reason == Some(SkippedReason::UnsupportedOperator)));
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
            Some(SkippedReason::UnsupportedOperator)
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
            .all(|result| result.skipped_reason == Some(SkippedReason::UnsupportedOperator)));
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
    fn run_validation_skips_distinct_operation_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000172.json"),
            r#"{
  "Core": { "Id": "CORE-000172", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
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
  "Outcome": { "Message": "STUDYID has oracle-specific distinct semantics" }
}"#,
        )
        .expect("write distinct oracle gap rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"]
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
        .expect("write distinct oracle gap data");

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
    fn run_validation_skips_domain_label_cat_oracle_gap_rule() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000272.json"),
            r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
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
  "Outcome": { "Message": "--CAT has oracle-specific domain label variable semantics" }
}"#,
        )
        .expect("write domain label oracle gap rule");

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
        "LBCAT": ["LB"]
      }
    }
  ]
}"#,
        )
        .expect("write domain label oracle gap data");

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
            Some(SkippedReason::OperationsNotSupported)
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
            Some(SkippedReason::UnsupportedOperator)
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
    fn run_validation_skips_operation_oracle_gap_rules() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-000891.json"),
            r#"{
  "Core": { "Id": "CORE-000891", "Status": "Published" },
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
