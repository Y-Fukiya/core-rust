use core_rule_model::{ExecutableRule, RuleType, Sensitivity};

use super::rule_id_has_oracle_gap_category;

pub(crate) fn is_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    !rule.operations.is_empty() && has_oracle_gap_rule_id(rule, "operation")
}

pub(crate) fn is_distinct_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_distinct_operation_oracle_gap(rule) {
        return false;
    }

    if crate::has_unsupported_reference_distinct_operation(rule)
        && !crate::is_supported_reference_distinct_rule(rule)
    {
        return true;
    }

    has_oracle_gap_rule_id(rule, "distinct_operation")
        && rule.operations.iter().any(|operation| {
            crate::operation_name(operation).as_deref() == Some("distinct")
                && !crate::bool_field(operation, &["value_is_reference"]).unwrap_or(false)
        })
}

pub(crate) fn is_dy_operation_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_dy_operation_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "dy_operation") && crate::has_dy_operation(rule)
}

pub(crate) fn is_required_value_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    rule.rule_type == RuleType::ValueLevelMetadata
        && (has_oracle_gap_rule_id(rule, "required_value_metadata")
            || has_oracle_gap_rule_id(rule, "official_oracle_fixture_gap"))
}

pub(crate) fn is_domain_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_domain_presence_oracle_gap(rule) {
        return false;
    }

    matches!(
        rule.rule_type,
        RuleType::DatasetMetadata | RuleType::DomainPresence
    ) && has_oracle_gap_rule_id(rule, "domain_presence")
}

pub(crate) fn is_variable_metadata_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_variable_metadata_oracle_gap(rule) {
        return false;
    }

    rule.rule_type == RuleType::VariableMetadata
        && has_oracle_gap_rule_id(rule, "variable_metadata")
}

pub(crate) fn is_domain_placeholder_column_ref_comparator_oracle_gap_rule(
    rule: &ExecutableRule,
) -> bool {
    if should_defer_domain_placeholder_column_ref_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "domain_placeholder_column_ref_comparator")
        && crate::contains_domain_placeholder_column_ref_comparator(&rule.conditions)
}

pub(crate) fn is_entity_literal_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    rule.entities.is_some() && has_oracle_gap_rule_id(rule, "entity_literal")
}

pub(crate) fn is_supported_entity_match_column_ref_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "supported_entity_match_column_ref")
}

pub(crate) fn is_empty_non_empty_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if std::env::var_os("CORE_RS_EXPERIMENT_ENABLE_EMPTY_NON_EMPTY").is_some() {
        return false;
    }

    if should_defer_empty_non_empty_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "empty_non_empty")
        && crate::contains_empty_operator(&rule.conditions)
}

pub(crate) fn is_date_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_date_operator_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "date_operator") && crate::contains_date_operator(&rule.conditions)
}

pub(crate) fn is_sort_operator_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_sort_operator_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "sort_operator") && crate::contains_sort_operator(&rule.conditions)
}

pub(crate) fn is_unique_set_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_unique_set_oracle_gap(rule) {
        return false;
    }
    has_oracle_gap_rule_id(rule, "unique_set")
        && crate::contains_unique_set_operator(&rule.conditions)
}

pub(crate) fn is_not_unique_relationship_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_not_unique_relationship_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "not_unique_relationship")
        && crate::contains_not_unique_relationship_operator(&rule.conditions)
}

pub(crate) fn is_inconsistent_across_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_inconsistent_across_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "inconsistent_across_dataset")
        && crate::contains_inconsistent_across_dataset_operator(&rule.conditions)
}

pub(crate) fn is_usdm_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "usdm_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

pub(crate) fn is_missing_column_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "missing_column")
}

pub(crate) fn is_multi_base_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_multi_base_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "multi_base_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

pub(crate) fn is_duplicate_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_duplicate_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "duplicate_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

pub(crate) fn is_relrec_or_supp_match_dataset_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    if should_defer_relrec_or_supp_match_dataset_oracle_gap(rule) {
        return false;
    }

    has_oracle_gap_rule_id(rule, "relrec_or_supp_match_dataset")
        && rule
            .datasets
            .as_ref()
            .is_some_and(|datasets| !datasets.is_empty())
}

pub(crate) fn should_defer_etcd_length_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_etcd_length")
        && crate::contains_longer_than_target(&rule.conditions, "ETCD")
        && crate::scope_matches(&crate::scope_values(rule.domains.as_ref(), "Include"), "SE")
}

pub(crate) fn should_defer_empty_non_empty_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_empty_non_empty")
        && crate::contains_empty_operator(&rule.conditions)
}

pub(crate) fn should_defer_positive_zero_oracle_gap_probe(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_positive_zero_probe")
}

pub(crate) fn should_defer_entity_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    rule.entities.is_some() && has_oracle_gap_rule_id(rule, "defer_entity_column_ref")
}

pub(crate) fn is_known_unsafe_positive_zero_probe_rule(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "unsafe_positive_zero_probe")
}

pub(crate) fn has_oracle_gap_rule_id(rule: &ExecutableRule, category: &str) -> bool {
    rule_id_has_oracle_gap_category(&rule.core_id, category)
}

fn should_defer_date_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_date_operator")
        && crate::contains_date_operator(&rule.conditions)
}

fn should_defer_dy_operation_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_dy_operation") && crate::has_dy_operation(rule)
}

fn should_defer_unique_set_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_unique_set")
        && crate::contains_unique_set_operator(&rule.conditions)
}

fn should_defer_sort_operator_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_sort_operator")
        && crate::contains_sort_operator(&rule.conditions)
}

fn should_defer_not_unique_relationship_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_not_unique_relationship")
        && crate::contains_not_unique_relationship_operator(&rule.conditions)
}

fn should_defer_inconsistent_across_dataset_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_inconsistent_across_dataset")
        && crate::contains_inconsistent_across_dataset_operator(&rule.conditions)
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

fn should_defer_domain_placeholder_column_ref_oracle_gap(rule: &ExecutableRule) -> bool {
    has_oracle_gap_rule_id(rule, "defer_domain_placeholder_column_ref")
        && crate::contains_domain_placeholder_column_ref_comparator(&rule.conditions)
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

pub(crate) fn is_dataset_presence_oracle_gap_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.sensitivity, Some(Sensitivity::Dataset))
        && rule.rule_type == RuleType::RecordData
        && crate::contains_presence_operator(&rule.conditions)
        && has_oracle_gap_rule_id(rule, "dataset_presence")
}
