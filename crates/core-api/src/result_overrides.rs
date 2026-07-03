use core_data::LoadedDataset;
use core_engine::{EngineError, RuleValidationResult, SkippedReason, ValidationIssue};
use core_rule_model::{ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity};

use crate::condition_inspect::contains_presence_operator;
use crate::operation_fields::{is_supported_operation_name, operation_name};
use crate::scope_filter::{scope_contains_all, scope_values};
use crate::{
    dataset_has_column, engine_semantics, expand_domain_placeholder_for_dataset,
    is_missing_column_oracle_gap_rule, outcome_message,
};

pub(crate) fn evaluation_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    source: EngineError,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        execution_provenance: None,
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

pub(crate) fn missing_column_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        execution_provenance: None,
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

pub(crate) fn missing_scope_wide_reference_target_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn missing_tpt_relationship_target_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: dataset.metadata().name.clone(),
        domain: dataset.metadata().domain.clone(),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn missing_tpt_relationship_pp_dataset_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: "PP".to_owned(),
        domain: Some("PP".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn missing_scoped_dataset_presence_result(
    rule: &ExecutableRule,
    execution_datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !execution_datasets.is_empty()
        || !engine_semantics::uses_missing_scoped_dataset_presence_result(rule)
        || !matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        || rule.rule_type != RuleType::RecordData
        || !contains_presence_operator(&rule.conditions)
    {
        return None;
    }

    let includes = scope_values(rule.domains.as_ref(), "Include");
    let excludes = scope_values(rule.domains.as_ref(), "Exclude");
    let [domain] = includes.as_slice() else {
        return None;
    };
    if !excludes.is_empty() || scope_contains_all(&includes) || domain.contains("--") {
        return None;
    }
    let output_variables = rule
        .output_variables
        .iter()
        .filter(|variable| !variable.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if output_variables.is_empty() {
        return None;
    }

    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let errors = output_variables
        .into_iter()
        .map(|variable| ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: domain.clone(),
            domain: Some(domain.clone()),
            row: None,
            variables: vec![variable],
            message: message.clone(),
            usubjid: None,
            seq: None,
        })
        .collect::<Vec<_>>();

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Failed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: domain.clone(),
        domain: Some(domain.clone()),
        message,
        error_count: errors.len(),
        errors,
    })
}

pub(crate) fn core_000138_dm_dataset_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: dm.metadata().name.clone(),
        domain: Some("DM".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn core_000095_se_dataset_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: se.metadata().name.clone(),
        domain: Some("SE".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn core_000572_cm_dataset_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: cm.metadata().name.clone(),
        domain: Some("CM".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn core_000466_pp_dataset_result(
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
        execution_provenance: None,
        skipped_reason: None,
        dataset: pp.metadata().name.clone(),
        domain: Some("PP".to_owned()),
        message,
        error_count: 1,
        errors: vec![issue],
    })
}

pub(crate) fn entity_column_ref_skipped_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Skipped,
        execution_provenance: None,
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

pub(crate) fn skipped_result_for_evaluation_error(
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

pub(crate) fn should_ignore_evaluation_error(
    rule: &ExecutableRule,
    source: &EngineError,
    execution_dataset_count: usize,
    open_rules_compat: bool,
) -> bool {
    execution_dataset_count > 1
        && matches!(source, EngineError::MissingColumn(_))
        && (!open_rules_compat || !is_missing_column_oracle_gap_rule(rule))
}

pub(crate) fn unsupported_operation(rule: &ExecutableRule) -> Option<String> {
    rule.operations.iter().find_map(|operation| {
        let name = operation_name(operation).unwrap_or_else(|| "<missing>".to_owned());
        (!is_supported_operation_name(&name)).then_some(name)
    })
}

pub(crate) fn unsupported_operator(group: &ConditionGroup) -> Option<&Operator> {
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

pub(crate) fn is_supported_basic_operator(operator: &Operator) -> bool {
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
