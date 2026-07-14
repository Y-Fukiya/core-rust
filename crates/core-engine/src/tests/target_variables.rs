use core_rule_model::{ConditionGroup, Operator, ValueExpr};

use super::super::*;
use super::common::{condition, literal};

#[test]
fn extracts_target_variables_from_nested_conditions() {
    let group = ConditionGroup::All(vec![
        ConditionGroup::Leaf(condition("AESEQ", Operator::EqualTo, literal(1))),
        ConditionGroup::Leaf(condition(
            "AESEQ",
            Operator::EqualTo,
            ValueExpr::ColumnRef("AESEQ_COPY".to_owned()),
        )),
        ConditionGroup::Not(Box::new(ConditionGroup::Leaf(condition(
            "DOMAIN",
            Operator::EqualTo,
            literal("AE"),
        )))),
    ]);

    assert_eq!(
        extract_target_variables(&group),
        vec!["AESEQ", "AESEQ_COPY", "DOMAIN"]
    );
}
