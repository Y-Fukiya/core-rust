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
    engine_semantics, expand_domain_placeholder_for_dataset, find_dataset, outcome_message,
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

pub(crate) fn core_000677_sequence_column(dataset: &LoadedDataset) -> Option<String> {
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
