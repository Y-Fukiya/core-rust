use core_rule_model::{ConditionGroup, ExecutableRule, OperationSpec, ValueExpr};

use crate::condition_inspect::contains_column_ref_comparator;
use crate::engine_semantics;
use crate::operation_fields::{bool_field, operation_name, string_field, string_list_field};
use crate::unsupported_operator;

pub(crate) fn has_reference_distinct_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("distinct" | "unique")
        ) && operation_dataset_name(operation).is_some()
            && string_field(operation, &["id", "target", "as", "output", "column"]).is_some()
    })
}

pub(crate) fn has_variable_count_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("variable_count"))
}

pub(crate) fn has_dataset_level_record_count_operation(rule: &ExecutableRule) -> bool {
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

pub(crate) fn is_supported_dataset_metadata_rule(rule: &ExecutableRule) -> bool {
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

pub(crate) fn is_supported_variable_metadata_rule(rule: &ExecutableRule) -> bool {
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

pub(crate) fn is_supported_value_metadata_rule(rule: &ExecutableRule) -> bool {
    engine_semantics::is_supported_value_metadata_rule_id(rule)
        && rule.operations.is_empty()
        && !contains_column_ref_comparator(&rule.conditions)
        && unsupported_operator(&rule.conditions).is_none()
}

pub(crate) fn has_expected_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("expected_variables"))
}

pub(crate) fn has_required_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("required_variables"))
}

pub(crate) fn has_dataset_column_order_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        operation_name(operation).as_deref() == Some("get_column_order_from_dataset")
    })
}

pub(crate) fn has_model_filtered_variables_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        operation_name(operation).as_deref() == Some("get_model_filtered_variables")
    })
}

pub(crate) fn has_model_column_order_operation(rule: &ExecutableRule) -> bool {
    engine_semantics::is_model_column_order_rule(rule)
        && rule.operations.iter().any(|operation| {
            matches!(
                operation_name(operation).as_deref(),
                Some("get_model_column_order" | "get_column_order_from_library")
            )
        })
}

pub(crate) fn has_variable_metadata_domain_prefix_operations(rule: &ExecutableRule) -> bool {
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

pub(crate) fn has_dy_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("dy"))
}

pub(crate) fn has_group_date_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("min_date" | "max_date")
        )
    })
}

pub(crate) fn has_match_dataset_dependent_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("map" | "codelist_extensible" | "codelist_terms")
        )
    })
}

pub(crate) fn has_group_aliases(operation: &OperationSpec) -> bool {
    string_list_field(operation, &["group_aliases", "groupAliases", "aliases"])
        .is_some_and(|aliases| !aliases.is_empty())
}

pub(crate) fn has_unsupported_reference_distinct_operation(rule: &ExecutableRule) -> bool {
    rule.operations.iter().any(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("distinct" | "unique")
        ) && operation_dataset_name(operation).is_some()
            && string_field(operation, &["id", "target", "as", "output", "column"]).is_some()
            && !bool_field(operation, &["value_is_reference"]).unwrap_or(false)
    })
}

pub(crate) fn operation_dataset_name(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["dataset", "domain", "input", "source"])
}

fn has_dataset_names_operation(rule: &ExecutableRule) -> bool {
    rule.operations
        .iter()
        .any(|operation| operation_name(operation).as_deref() == Some("dataset_names"))
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
    core_rule_model::normalize_key(variable).starts_with("library_")
}
