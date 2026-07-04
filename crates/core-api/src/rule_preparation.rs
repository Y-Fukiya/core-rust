use core_rule_model::{Condition, ConditionGroup, ExecutableRule, Operator, ValueExpr};
use serde_json::Value;

use crate::cdisc_context::{apply_cdisc_context_to_group, CdiscContext};
use crate::engine_semantics;
use crate::report_variables::{
    apply_metadata_report_variables, apply_operation_report_variables,
    apply_requested_standard_operation_semantics,
};
use crate::usdm_hand_ports::apply_usdm_hand_port_semantics;

pub(crate) fn prepare_rule_for_execution(
    rule: &ExecutableRule,
    context: &CdiscContext,
    standard: &Option<String>,
) -> ExecutableRule {
    let mut rule = prepare_rule_with_cdisc_context(rule, context);
    apply_usdm_hand_port_semantics(&mut rule);
    apply_open_rules_relationship_semantics(&mut rule);
    apply_trial_summary_value_null_flavor_semantics(&mut rule);
    apply_unscheduled_death_ds_flag_semantics(&mut rule);
    apply_requested_standard_operation_semantics(&mut rule, standard);
    apply_entity_instance_type_literals(&mut rule);
    apply_metadata_report_variables(&mut rule);
    apply_operation_report_variables(&mut rule);
    apply_alphanumeric_fa_split_dataset_name_regex(&mut rule);
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

fn apply_alphanumeric_fa_split_dataset_name_regex(rule: &mut ExecutableRule) {
    if !engine_semantics::uses_alphanumeric_fa_split_dataset_name_regex(rule) {
        return;
    }

    rewrite_dataset_name_matches_regex(&mut rule.conditions, "(?i)^[a-z]{2}[a-z0-9]{1,2}");
}

fn rewrite_dataset_name_matches_regex(group: &mut ConditionGroup, pattern: &str) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                rewrite_dataset_name_matches_regex(group, pattern);
            }
        }
        ConditionGroup::Not(group) => rewrite_dataset_name_matches_regex(group, pattern),
        ConditionGroup::Leaf(condition) => {
            if condition
                .target
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case("dataset_name"))
                && condition.operator == Operator::MatchesRegex
            {
                condition.comparator = ValueExpr::Literal(Value::String(pattern.to_owned()));
            }
        }
    }
}

fn apply_unscheduled_death_ds_flag_semantics(rule: &mut ExecutableRule) {
    if !engine_semantics::is_unscheduled_death_ds_flag_rule(rule) {
        return;
    }

    rule.conditions = ConditionGroup::All(vec![
        non_empty_condition("DDTESTCD"),
        ConditionGroup::Leaf(Condition {
            target: Some("DSUSCHFL".to_owned()),
            operator: Operator::IsEmpty,
            comparator: ValueExpr::Null,
            options: Default::default(),
        }),
    ]);
    rule.output_variables = vec!["DDTESTCD".to_owned(), "DSUSCHFL".to_owned()];
}

fn apply_trial_summary_value_null_flavor_semantics(rule: &mut ExecutableRule) {
    if !engine_semantics::is_trial_summary_null_flavor_rule(rule) {
        return;
    }

    rule.conditions = ConditionGroup::All(vec![
        non_empty_condition("TSVAL"),
        non_empty_condition("TSVALNF"),
    ]);
}

fn apply_open_rules_relationship_semantics(rule: &mut ExecutableRule) {
    if let Some(direction) = engine_semantics::open_rules_relationship_direction(rule) {
        set_not_unique_relationship_direction(&mut rule.conditions, direction);
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

fn non_empty_condition(target: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator: Operator::IsNotEmpty,
        comparator: ValueExpr::Null,
        options: Default::default(),
    })
}

pub(crate) fn apply_entity_instance_type_literals(rule: &mut ExecutableRule) {
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
