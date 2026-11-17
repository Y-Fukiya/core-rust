use crate::operation_fields::operation_name;
use crate::{
    collect_condition_target_variables, condition_targets_column, engine_semantics,
    has_reference_distinct_operation, push_unique_string,
};
use core_rule_model::{ExecutableRule, RuleType};

pub(crate) fn apply_operation_report_variables(rule: &mut ExecutableRule) {
    if engine_semantics::is_requested_standard_operation_rule(rule)
        && rule.output_variables.is_empty()
        && rule
            .operations
            .iter()
            .any(|operation| operation_name(operation).as_deref() == Some("domain_label"))
    {
        collect_condition_target_variables(&rule.conditions, &mut rule.output_variables);
    }

    if engine_semantics::uses_check_target_report_variable_for_ex_end_rule(rule)
        && condition_targets_column(&rule.conditions, "RFXENDTC")
    {
        for variable in &mut rule.output_variables {
            if variable.eq_ignore_ascii_case("RFXSTDTC") {
                *variable = "RFXENDTC".to_owned();
            }
        }
    }

    if engine_semantics::is_operation_report_variable_override_rule(rule)
        && rule
            .output_variables
            .iter()
            .any(|variable| variable == "$model_variables")
    {
        push_unique_string(&mut rule.output_variables, "USUBJID");
        push_unique_string(&mut rule.output_variables, "STUDYID");
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

pub(crate) fn apply_metadata_report_variables(rule: &mut ExecutableRule) {
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
