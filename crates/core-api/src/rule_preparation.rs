use core_rule_model::{ConditionGroup, ExecutableRule, Operator, ValueExpr};
use serde_json::Value;

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
