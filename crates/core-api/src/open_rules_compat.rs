//! Open Rules oracle-compatibility helpers.
//!
//! This module is intentionally separate from the generic validation path. Open
//! Rules compatibility gaps are coverage decisions for the oracle harness, not
//! production engine semantics.

use core_engine::RuleValidationResult;
use core_rule_model::ExecutableRule;

const HAND_PORT_RULE_ID_MANIFEST: &str = include_str!("open_rules_compat/hand_port_rule_ids.csv");

pub fn rule_id_uses_hand_port(rule_id: &str) -> bool {
    let rule_id = rule_id.trim();
    !rule_id.is_empty() && hand_port_rule_ids().any(|manifest_rule_id| manifest_rule_id == rule_id)
}

fn hand_port_rule_ids() -> impl Iterator<Item = &'static str> {
    HAND_PORT_RULE_ID_MANIFEST
        .lines()
        .filter_map(parse_hand_port_manifest_rule_id)
}

fn parse_hand_port_manifest_rule_id(line: &'static str) -> Option<&'static str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("rule_id,") {
        return None;
    }
    line.split(',')
        .next()
        .map(str::trim)
        .filter(|rule_id| !rule_id.is_empty())
}

pub(crate) fn post_execution_oracle_gap_result(
    rule: &ExecutableRule,
    result: &RuleValidationResult,
) -> Option<RuleValidationResult> {
    let _ = (rule, result);
    // Do not rewrite executed engine output into skipped oracle-gap rows. Keeping
    // failures as failures preserves scoreboard independence.
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn hand_port_rule_ids_are_loaded_from_manifest() {
        assert!(rule_id_uses_hand_port("CORE-000583"));
        assert!(rule_id_uses_hand_port(" CORE-001077 "));
        assert!(!rule_id_uses_hand_port("CORE-PROV"));
        assert!(!rule_id_uses_hand_port(""));
    }

    #[test]
    fn hand_port_manifest_has_unique_rule_ids() {
        let mut seen = BTreeSet::new();
        let mut count = 0;
        for rule_id in hand_port_rule_ids() {
            assert!(
                seen.insert(rule_id),
                "duplicate hand-port rule id {rule_id}"
            );
            count += 1;
        }
        assert!(count > 100, "unexpectedly small hand-port manifest");
    }
}
