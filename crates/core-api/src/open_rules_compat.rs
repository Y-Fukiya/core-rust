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
const ORACLE_GAP_RULE_ID_MANIFEST: &str = include_str!("open_rules_compat/oracle_gap_rule_ids.csv");
const ORACLE_GAP_RULE_ID_HEADER: &str = "rule_id,category,reason,owner,evidence,scope";
const EXPECTED_ORACLE_GAP_RULE_ID_COUNT: usize = 425;
const ORACLE_GAP_SCOPE: &str = "open-rules-oracle-harness";
#[cfg(test)]
const RULE_SPECIFIC_SEMANTICS_MANIFEST: &str =
    include_str!("open_rules_compat/rule_specific_semantics.csv");
#[cfg(test)]
const RULE_SPECIFIC_SEMANTICS_HEADER: &str =
    "rule_id,classification,category,reason,owner,evidence,scope";
#[cfg(test)]
const EXPECTED_RULE_SPECIFIC_SEMANTICS_RULE_ID_COUNT: usize = 209;

static HAND_PORT_RULE_IDS: LazyLock<BTreeSet<&'static str>> =
    LazyLock::new(load_hand_port_rule_ids);
static ORACLE_GAP_RULE_IDS: LazyLock<BTreeSet<(&'static str, &'static str)>> =
    LazyLock::new(load_oracle_gap_rule_ids);

pub fn rule_id_uses_hand_port(rule_id: &str) -> bool {
    let rule_id = rule_id.trim();
    !rule_id.is_empty() && HAND_PORT_RULE_IDS.contains(rule_id)
}

pub(crate) fn oracle_gap_rule_id_matches(rule_id: &str, category: &str) -> bool {
    let rule_id = rule_id.trim();
    let category = category.trim();
    !rule_id.is_empty()
        && !category.is_empty()
        && ORACLE_GAP_RULE_IDS.contains(&(category, rule_id))
}

#[cfg(test)]
fn hand_port_rule_ids() -> impl Iterator<Item = &'static str> {
    HAND_PORT_RULE_IDS.iter().copied()
}

#[cfg(test)]
fn oracle_gap_rule_ids() -> impl Iterator<Item = (&'static str, &'static str)> {
    ORACLE_GAP_RULE_IDS.iter().copied()
}

#[cfg(test)]
fn rule_specific_semantics_rule_ids() -> impl Iterator<Item = &'static str> {
    load_rule_specific_semantics_rule_ids_from_manifest(RULE_SPECIFIC_SEMANTICS_MANIFEST)
        .into_iter()
}

fn load_hand_port_rule_ids() -> BTreeSet<&'static str> {
    validate_hand_port_manifest_header(HAND_PORT_RULE_ID_MANIFEST);
    let mut rule_ids = BTreeSet::new();
    for rule_id in parse_hand_port_manifest_rule_ids(HAND_PORT_RULE_ID_MANIFEST) {
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

fn load_oracle_gap_rule_ids() -> BTreeSet<(&'static str, &'static str)> {
    let rule_ids = load_oracle_gap_rule_ids_from_manifest(ORACLE_GAP_RULE_ID_MANIFEST);
    assert_eq!(
        rule_ids.len(),
        EXPECTED_ORACLE_GAP_RULE_ID_COUNT,
        "unexpected oracle-gap manifest rule count"
    );
    rule_ids
}

fn load_oracle_gap_rule_ids_from_manifest(
    manifest: &'static str,
) -> BTreeSet<(&'static str, &'static str)> {
    validate_oracle_gap_manifest_header(manifest);
    let mut rule_ids = BTreeSet::new();
    for (category, rule_id) in parse_oracle_gap_manifest_rule_ids(manifest) {
        assert!(
            rule_ids.insert((category, rule_id)),
            "duplicate oracle-gap rule id {rule_id} in category {category}"
        );
    }
    rule_ids
}

#[cfg(test)]
fn load_rule_specific_semantics_rule_ids_from_manifest(
    manifest: &'static str,
) -> BTreeSet<&'static str> {
    validate_rule_specific_semantics_manifest_header(manifest);
    let mut rule_ids = BTreeSet::new();
    for rule_id in parse_rule_specific_semantics_rule_ids(manifest) {
        assert!(
            rule_ids.insert(rule_id),
            "duplicate rule-specific semantics rule id {rule_id}"
        );
    }
    assert_eq!(
        rule_ids.len(),
        EXPECTED_RULE_SPECIFIC_SEMANTICS_RULE_ID_COUNT,
        "unexpected rule-specific semantics manifest rule count"
    );
    rule_ids
}

fn validate_hand_port_manifest_header(manifest: &str) {
    let header = manifest
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

fn validate_oracle_gap_manifest_header(manifest: &str) {
    let header = manifest
        .lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then_some(line)
        })
        .expect("oracle-gap manifest must include a header");
    assert_eq!(
        header, ORACLE_GAP_RULE_ID_HEADER,
        "invalid oracle-gap manifest header"
    );
}

#[cfg(test)]
fn validate_rule_specific_semantics_manifest_header(manifest: &str) {
    let header = manifest
        .lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then_some(line)
        })
        .expect("rule-specific semantics manifest must include a header");
    assert_eq!(
        header, RULE_SPECIFIC_SEMANTICS_HEADER,
        "invalid rule-specific semantics manifest header"
    );
}

fn parse_hand_port_manifest_rule_ids(manifest: &'static str) -> impl Iterator<Item = &'static str> {
    let mut header_seen = false;
    manifest
        .lines()
        .enumerate()
        .filter_map(move |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            if trimmed == HAND_PORT_RULE_ID_HEADER {
                assert!(
                    !header_seen,
                    "duplicate hand-port manifest header at row {}",
                    index + 1
                );
                header_seen = true;
                return None;
            }
            parse_hand_port_manifest_rule_id((index, line))
        })
}

fn parse_oracle_gap_manifest_rule_ids(
    manifest: &'static str,
) -> impl Iterator<Item = (&'static str, &'static str)> {
    let mut header_seen = false;
    manifest
        .lines()
        .enumerate()
        .filter_map(move |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            if trimmed == ORACLE_GAP_RULE_ID_HEADER {
                assert!(
                    !header_seen,
                    "duplicate oracle-gap manifest header at row {}",
                    index + 1
                );
                header_seen = true;
                return None;
            }
            parse_oracle_gap_manifest_rule_id((index, line))
        })
}

#[cfg(test)]
fn parse_rule_specific_semantics_rule_ids(
    manifest: &'static str,
) -> impl Iterator<Item = &'static str> {
    let mut header_seen = false;
    manifest
        .lines()
        .enumerate()
        .filter_map(move |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            if trimmed == RULE_SPECIFIC_SEMANTICS_HEADER {
                assert!(
                    !header_seen,
                    "duplicate rule-specific semantics manifest header at row {}",
                    index + 1
                );
                header_seen = true;
                return None;
            }
            parse_rule_specific_semantics_rule_id((index, line))
        })
}

fn parse_hand_port_manifest_rule_id((index, line): (usize, &'static str)) -> Option<&'static str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
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

fn parse_oracle_gap_manifest_rule_id(
    (index, line): (usize, &'static str),
) -> Option<(&'static str, &'static str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    assert_eq!(
        fields.len(),
        6,
        "invalid oracle-gap manifest row {}: expected 6 columns",
        index + 1
    );
    let rule_id = fields[0];
    let category = fields[1];
    let reason = fields[2];
    let owner = fields[3];
    let evidence = fields[4];
    let scope = fields[5];
    assert!(
        is_core_rule_id(rule_id),
        "invalid oracle-gap rule id in manifest row {}: {rule_id}",
        index + 1
    );
    assert!(
        !category.is_empty(),
        "missing oracle-gap category for rule id {rule_id}"
    );
    assert!(
        !reason.is_empty(),
        "missing oracle-gap reason for rule id {rule_id}"
    );
    assert!(
        !owner.is_empty(),
        "missing owner for oracle-gap rule id {rule_id}"
    );
    assert!(
        !evidence.is_empty(),
        "missing evidence for oracle-gap rule id {rule_id}"
    );
    assert_eq!(
        scope, ORACLE_GAP_SCOPE,
        "invalid oracle-gap scope for {rule_id}"
    );
    Some((category, rule_id))
}

#[cfg(test)]
fn parse_rule_specific_semantics_rule_id(
    (index, line): (usize, &'static str),
) -> Option<&'static str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    assert_eq!(
        fields.len(),
        7,
        "invalid rule-specific semantics manifest row {}: expected 7 columns",
        index + 1
    );
    let rule_id = fields[0];
    let classification = fields[1];
    let category = fields[2];
    let reason = fields[3];
    let owner = fields[4];
    let evidence = fields[5];
    let scope = fields[6];
    assert!(
        is_core_rule_id(rule_id),
        "invalid rule-specific semantics rule id in manifest row {}: {rule_id}",
        index + 1
    );
    assert!(
        !classification.is_empty(),
        "missing rule-specific semantics classification for rule id {rule_id}"
    );
    assert!(
        !category.is_empty(),
        "missing rule-specific semantics category for rule id {rule_id}"
    );
    assert!(
        !reason.is_empty(),
        "missing rule-specific semantics reason for rule id {rule_id}"
    );
    assert!(
        !owner.is_empty(),
        "missing owner for rule-specific semantics rule id {rule_id}"
    );
    assert!(
        !evidence.is_empty(),
        "missing evidence for rule-specific semantics rule id {rule_id}"
    );
    assert_eq!(
        scope, ORACLE_GAP_SCOPE,
        "invalid rule-specific semantics scope for {rule_id}"
    );
    Some(rule_id)
}

#[cfg(test)]
fn core_rule_ids_in(source: &str) -> BTreeSet<&str> {
    let mut rule_ids = BTreeSet::new();
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find("CORE-") {
        let start = cursor + offset;
        let end = start + "CORE-000000".len();
        if end <= source.len() {
            let candidate = &source[start..end];
            if is_core_rule_id(candidate) {
                rule_ids.insert(candidate);
            }
        }
        cursor = start + "CORE-".len();
    }
    rule_ids
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

    #[test]
    #[should_panic(expected = "duplicate hand-port manifest header")]
    fn hand_port_manifest_rejects_duplicate_header() {
        parse_hand_port_manifest_rule_ids(
            "rule_id,execution_provenance,owner,scope\n\
CORE-000047,rule_id_hand_port,core-api,open-rules-oracle-harness\n\
rule_id,execution_provenance,owner,scope\n\
CORE-000095,rule_id_hand_port,core-api,open-rules-oracle-harness\n",
        )
        .for_each(drop);
    }

    #[test]
    fn oracle_gap_rule_ids_are_loaded_from_manifest() {
        assert!(oracle_gap_rule_id_matches("CORE-000773", "operation"));
        assert!(oracle_gap_rule_id_matches(
            " CORE-000195 ",
            "defer_domain_placeholder_column_ref"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000098",
            "defer_positive_zero_probe"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000545",
            "unsafe_positive_zero_probe"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000427",
            "supported_entity_match_column_ref"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000036",
            "supported_reference_distinct"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000140",
            "scope_wide_reference_distinct"
        ));
        assert!(oracle_gap_rule_id_matches(
            "CORE-000200",
            "missing_condition_columns_as_null"
        ));
        assert!(!oracle_gap_rule_id_matches("CORE-000773", "date_operator"));
        assert!(!oracle_gap_rule_id_matches("CORE-PROV", "operation"));
        assert!(!oracle_gap_rule_id_matches("", "operation"));
    }

    #[test]
    fn oracle_gap_manifest_has_unique_rule_ids_per_category() {
        let mut seen = BTreeSet::new();
        let mut count = 0;
        for (category, rule_id) in oracle_gap_rule_ids() {
            assert!(
                seen.insert((category, rule_id)),
                "duplicate oracle-gap rule id {rule_id} in category {category}"
            );
            count += 1;
        }
        assert_eq!(count, EXPECTED_ORACLE_GAP_RULE_ID_COUNT);
    }

    #[test]
    #[should_panic(expected = "invalid oracle-gap manifest row")]
    fn oracle_gap_manifest_rejects_wrong_column_count() {
        parse_oracle_gap_manifest_rule_id((
            1,
            "CORE-000773,operation,reason,core-api,evidence,open-rules-oracle-harness,extra",
        ));
    }

    #[test]
    #[should_panic(expected = "invalid oracle-gap rule id")]
    fn oracle_gap_manifest_rejects_invalid_rule_id() {
        parse_oracle_gap_manifest_rule_id((
            1,
            "NOT-A-CORE-ID,operation,reason,core-api,evidence,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "missing oracle-gap category")]
    fn oracle_gap_manifest_rejects_missing_category() {
        parse_oracle_gap_manifest_rule_id((
            1,
            "CORE-000773,,reason,core-api,evidence,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "missing oracle-gap reason")]
    fn oracle_gap_manifest_rejects_missing_reason() {
        parse_oracle_gap_manifest_rule_id((
            1,
            "CORE-000773,operation,,core-api,evidence,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "missing evidence")]
    fn oracle_gap_manifest_rejects_missing_evidence() {
        parse_oracle_gap_manifest_rule_id((
            1,
            "CORE-000773,operation,reason,core-api,,open-rules-oracle-harness",
        ));
    }

    #[test]
    #[should_panic(expected = "duplicate oracle-gap manifest header")]
    fn oracle_gap_manifest_rejects_duplicate_header() {
        parse_oracle_gap_manifest_rule_ids(
            "rule_id,category,reason,owner,evidence,scope\n\
CORE-000773,operation,reason,core-api,evidence,open-rules-oracle-harness\n\
rule_id,category,reason,owner,evidence,scope\n\
CORE-001034,operation,reason,core-api,evidence,open-rules-oracle-harness\n",
        )
        .for_each(drop);
    }

    #[test]
    #[should_panic(expected = "duplicate oracle-gap rule id")]
    fn oracle_gap_manifest_rejects_duplicate_rule_id_in_category() {
        load_oracle_gap_rule_ids_from_manifest(
            "rule_id,category,reason,owner,evidence,scope\n\
CORE-000773,operation,reason,core-api,evidence,open-rules-oracle-harness\n\
CORE-000773,operation,reason,core-api,evidence,open-rules-oracle-harness\n",
        );
    }

    #[test]
    fn rule_specific_semantics_manifest_covers_core_api_hard_coded_rule_ids() {
        let mut hard_coded = core_rule_ids_in(include_str!("lib.rs"));
        hard_coded.extend(core_rule_ids_in(include_str!("engine_semantics.rs")));
        hard_coded.extend(core_rule_ids_in(include_str!("standard_filter.rs")));
        hard_coded.extend(core_rule_ids_in(include_str!("usdm_jsonata.rs")));
        let classified = rule_specific_semantics_rule_ids().collect::<BTreeSet<_>>();
        let missing = hard_coded
            .difference(&classified)
            .copied()
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "rule-specific semantics manifest is missing {missing:?}"
        );
    }

    #[test]
    fn usdm_hand_port_semantics_are_isolated_outside_core_api_lib() {
        let lib_rule_ids = core_rule_ids_in(include_str!("lib.rs"));
        let hand_port = RULE_SPECIFIC_SEMANTICS_MANIFEST
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                (fields.len() == 7 && fields[1] == "hand_port_semantics").then_some(fields[0])
            })
            .collect::<BTreeSet<_>>();
        let leaked = lib_rule_ids
            .intersection(&hand_port)
            .copied()
            .collect::<Vec<_>>();
        assert!(
            leaked.is_empty(),
            "USDM hand-port semantics still live in lib.rs: {leaked:?}"
        );
    }

    #[test]
    fn engine_semantics_rule_ids_are_isolated_outside_core_api_lib() {
        let lib_rule_ids = core_rule_ids_in(include_str!("lib.rs"));
        let engine_semantics = RULE_SPECIFIC_SEMANTICS_MANIFEST
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
                (fields.len() == 7 && fields[1] == "engine_semantics").then_some(fields[0])
            })
            .collect::<BTreeSet<_>>();
        let leaked = lib_rule_ids
            .intersection(&engine_semantics)
            .copied()
            .collect::<Vec<_>>();
        assert!(
            leaked.is_empty(),
            "engine semantics still live in lib.rs: {leaked:?}"
        );
    }
}
