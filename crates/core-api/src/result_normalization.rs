use std::collections::BTreeSet;

use core_data::{dataset_column_values, DatasetSourceFormat, LoadedDataset};
use core_engine::{evaluate_condition_group, RuleValidationResult, SkippedReason, ValidationIssue};
use core_rule_model::{ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity, ValueExpr};
use serde_json::Value;

use crate::dataset_helpers::{
    dataset_column_name, dataset_domain_value, dataset_has_column, dataset_metadata_name,
    push_unique_string,
};
use crate::json_values::json_scalar_string;
use crate::metadata_support::{
    has_dataset_level_record_count_operation, has_variable_count_operation,
};
use crate::{
    dataset_matches_name, engine_semantics, expand_domain_placeholder_for_dataset, find_dataset,
    outcome_message,
};

pub(crate) fn normalize_validation_result(
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

pub(crate) fn dataset_cell_string(values: &[Value], row: usize) -> String {
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

pub(crate) fn core_000206_idvarval_rdomain_result(
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

pub(crate) fn core_000677_pooldef_poolid_result(
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

pub(crate) fn core_000884_ts_age_parameter_count_result(
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
            variables: vec!["DOMAIN".to_owned(), "$ageu_count".to_owned()],
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

pub(crate) fn core_000744_relrec_faobj_result(
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

pub(crate) fn core_000757_intervention_relrec_faobj_result(
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
