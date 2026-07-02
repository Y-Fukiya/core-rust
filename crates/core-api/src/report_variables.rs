use core_rule_model::{ExecutableRule, RuleType};
use serde_json::Value;

use crate::operation_fields::operation_name;
use crate::{
    collect_condition_target_variables, condition_targets_column, engine_semantics,
    has_reference_distinct_operation, push_unique_string,
};

pub(crate) fn apply_operation_report_variables(rule: &mut ExecutableRule) {
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

pub(crate) fn apply_requested_standard_operation_semantics(
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
