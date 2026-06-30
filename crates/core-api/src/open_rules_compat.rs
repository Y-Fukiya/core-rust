//! Open Rules oracle-compatibility helpers.
//!
//! This module is intentionally separate from the generic validation path. Open
//! Rules compatibility gaps are coverage decisions for the oracle harness, not
//! production engine semantics.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use core_engine::RuleValidationResult;
use core_rule_model::ExecutableRule;

const HAND_PORT_RULE_ID_MANIFEST: &str = include_str!("open_rules_compat/hand_port_rule_ids.csv");
const HAND_PORT_RULE_ID_HEADER: &str = "rule_id,execution_provenance,owner,scope";
const EXPECTED_HAND_PORT_RULE_ID_COUNT: usize = 119;
const HAND_PORT_PROVENANCE: &str = "rule_id_hand_port";
const HAND_PORT_SCOPE: &str = "open-rules-oracle-harness";

static HAND_PORT_RULE_IDS: LazyLock<BTreeSet<&'static str>> =
    LazyLock::new(load_hand_port_rule_ids);

pub fn rule_id_uses_hand_port(rule_id: &str) -> bool {
    let rule_id = rule_id.trim();
    !rule_id.is_empty() && HAND_PORT_RULE_IDS.contains(rule_id)
}

#[cfg(test)]
fn hand_port_rule_ids() -> impl Iterator<Item = &'static str> {
    HAND_PORT_RULE_IDS.iter().copied()
}

fn load_hand_port_rule_ids() -> BTreeSet<&'static str> {
    validate_hand_port_manifest_header();
    let mut rule_ids = BTreeSet::new();
    for rule_id in HAND_PORT_RULE_ID_MANIFEST
        .lines()
        .enumerate()
        .filter_map(parse_hand_port_manifest_rule_id)
    {
        assert!(
            rule_ids.insert(rule_id),
            "duplicate hand-port rule id {rule_id}"
        );
    }
    assert_eq!(
        rule_ids.len(),
        EXPECTED_HAND_PORT_RULE_ID_COUNT,
        "unexpected hand-port manifest rule count"
    );
    rule_ids
}

fn validate_hand_port_manifest_header() {
    let header = HAND_PORT_RULE_ID_MANIFEST
        .lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then_some(line)
        })
        .expect("hand-port manifest must include a header");
    assert_eq!(
        header, HAND_PORT_RULE_ID_HEADER,
        "invalid hand-port manifest header"
    );
}

fn parse_hand_port_manifest_rule_id((index, line): (usize, &'static str)) -> Option<&'static str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line == HAND_PORT_RULE_ID_HEADER {
        return None;
    }
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    assert_eq!(
        fields.len(),
        4,
        "invalid hand-port manifest row {}: expected 4 columns",
        index + 1
    );
    let rule_id = fields[0];
    let provenance = fields[1];
    let owner = fields[2];
    let scope = fields[3];
    assert!(
        is_core_rule_id(rule_id),
        "invalid hand-port rule id in manifest row {}: {rule_id}",
        index + 1
    );
    assert_eq!(
        provenance, HAND_PORT_PROVENANCE,
        "invalid hand-port provenance for {rule_id}"
    );
    assert!(
        !owner.is_empty(),
        "missing owner for hand-port rule id {rule_id}"
    );
    assert_eq!(
        scope, HAND_PORT_SCOPE,
        "invalid hand-port scope for {rule_id}"
    );
    Some(rule_id)
}

fn is_core_rule_id(rule_id: &str) -> bool {
    let Some(digits) = rule_id.strip_prefix("CORE-") else {
        return false;
    };
    digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_digit())
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
        assert_eq!(count, EXPECTED_HAND_PORT_RULE_ID_COUNT);
    }

    #[test]
    #[should_panic(expected = "invalid hand-port manifest row")]
    fn hand_port_manifest_rejects_wrong_column_count() {
        parse_hand_port_manifest_rule_id((
            1,
            "CORE-000583,rule_id_hand_port,core-api,open-rules-oracle-harness,extra",
        ));
    }

    #[test]
    #[should_panic(expected = "invalid hand-port rule id")]
    fn hand_port_manifest_rejects_invalid_rule_id() {
        parse_hand_port_manifest_rule_id((
            1,
            "NOT-A-CORE-ID,rule_id_hand_port,core-api,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "invalid hand-port provenance")]
    fn hand_port_manifest_rejects_invalid_provenance() {
        parse_hand_port_manifest_rule_id((
            1,
            "CORE-000583,native_engine,core-api,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "missing owner")]
    fn hand_port_manifest_rejects_missing_owner() {
        parse_hand_port_manifest_rule_id((
            1,
            "CORE-000583,rule_id_hand_port,,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "invalid hand-port scope")]
    fn hand_port_manifest_rejects_invalid_scope() {
        parse_hand_port_manifest_rule_id((1, "CORE-000583,rule_id_hand_port,core-api,wrong-scope"));
    }
}
