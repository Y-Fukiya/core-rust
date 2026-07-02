use core_engine::{RuleValidationResult, SkippedReason};
use core_rule_model::{ExecutableRule, StandardRef};

use crate::RuleSelection;

pub(crate) fn apply_standard_filter(
    selection: &mut RuleSelection,
    include_rules: &[String],
    standard: &Option<String>,
    standard_version: &Option<String>,
) {
    if standard.is_none() {
        return;
    }

    let mut selected = Vec::with_capacity(selection.selected.len());
    for rule in std::mem::take(&mut selection.selected) {
        if rule_matches_standard(&rule, standard, standard_version) {
            selected.push(rule);
        } else if !include_rules.is_empty() {
            selection.skipped.push(standard_mismatch_result(
                &rule,
                standard.as_deref(),
                standard_version.as_deref(),
            ));
        }
    }
    selection.selected = selected;
}

pub(crate) fn apply_standard_oracle_gap_filter(
    selection: &mut RuleSelection,
    standard: &Option<String>,
    standard_version: &Option<String>,
) {
    let mut selected = Vec::with_capacity(selection.selected.len());
    for rule in std::mem::take(&mut selection.selected) {
        if is_sendig_31_operation_oracle_gap(&rule, standard, standard_version) {
            selection.skipped.push(RuleValidationResult::skipped_rule(
                rule.core_id.clone(),
                SkippedReason::OracleSemanticsGap,
                format!(
                    "Rule {} uses SENDIG 3.1 operation oracle semantics that are not supported",
                    rule.core_id
                ),
            ));
        } else {
            selected.push(rule);
        }
    }
    selection.selected = selected;
}

fn rule_matches_standard(
    rule: &ExecutableRule,
    standard: &Option<String>,
    standard_version: &Option<String>,
) -> bool {
    let Some(standard) = standard.as_deref() else {
        return true;
    };

    rule.standards.iter().any(|rule_standard| {
        rule_standard_matches_name(rule_standard, standard, &rule.core_id)
            && standard_version.as_deref().is_none_or(|version| {
                rule_standard
                    .version
                    .as_deref()
                    .is_some_and(|rule_version| {
                        rule_version.eq_ignore_ascii_case(version)
                            || standard_version_compatible(standard, version, rule_version)
                    })
            })
    })
}

fn rule_standard_matches_name(rule_standard: &StandardRef, requested: &str, rule_id: &str) -> bool {
    if rule_id == "CORE-000478" && requested.eq_ignore_ascii_case("SENDIG") {
        return false;
    }

    if rule_id == "CORE-000119"
        && requested.eq_ignore_ascii_case("SENDIG")
        && rule_standard
            .name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("TIG"))
        && rule_standard.extra.get("Substandard").is_some_and(|value| {
            value
                .as_str()
                .is_some_and(|substandard| substandard.eq_ignore_ascii_case("SDTM"))
        })
    {
        return true;
    }

    rule_standard
        .name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(requested))
        || (requested.eq_ignore_ascii_case("SENDIG")
            && rule_standard.name.as_deref().is_some_and(|name| {
                matches!(
                    name.to_ascii_uppercase().as_str(),
                    "SENDIG-DART" | "SENDIG-GENETOX"
                )
            }))
        || (requested.eq_ignore_ascii_case("SDTMIG")
            && rule_standard
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("TIG"))
            && rule_standard.extra.get("Substandard").is_some_and(|value| {
                value
                    .as_str()
                    .is_some_and(|substandard| substandard.eq_ignore_ascii_case("SDTM"))
            }))
}

fn standard_version_compatible(standard: &str, requested: &str, rule_version: &str) -> bool {
    (standard.eq_ignore_ascii_case("USDM") && requested == "4.0" && rule_version == "3.0")
        || (standard.eq_ignore_ascii_case("SDTMIG") && requested == "3.3" && rule_version == "3.4")
        || (standard.eq_ignore_ascii_case("SDTMIG") && requested == "3.4" && rule_version == "1.0")
        || (standard.eq_ignore_ascii_case("SENDIG")
            && matches!(requested, "3.0" | "3.1")
            && matches!(
                rule_version,
                "1.0" | "1.1" | "1.2" | "3.0" | "3.1" | "3.1.1"
            ))
}

fn is_sendig_31_operation_oracle_gap(
    rule: &ExecutableRule,
    standard: &Option<String>,
    standard_version: &Option<String>,
) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000172" | "CORE-000770" | "CORE-000884"
    ) && standard
        .as_deref()
        .is_some_and(|standard| standard.eq_ignore_ascii_case("SENDIG"))
        && standard_version
            .as_deref()
            .is_some_and(|version| version.eq_ignore_ascii_case("3.1"))
        && !rule.operations.is_empty()
}

fn standard_mismatch_result(
    rule: &ExecutableRule,
    standard: Option<&str>,
    standard_version: Option<&str>,
) -> RuleValidationResult {
    let requested = match (standard, standard_version) {
        (Some(standard), Some(version)) => format!("{standard} {version}"),
        (Some(standard), None) => standard.to_owned(),
        _ => "requested standard".to_owned(),
    };
    let reason = if is_standard_filter_oracle_gap_rule(rule, standard, standard_version) {
        SkippedReason::OracleSemanticsGap
    } else {
        SkippedReason::StandardMismatch
    };

    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        reason,
        format!(
            "Requested rule {} does not match standard filter {}",
            rule.core_id, requested
        ),
    )
}

fn is_standard_filter_oracle_gap_rule(
    rule: &ExecutableRule,
    standard: Option<&str>,
    standard_version: Option<&str>,
) -> bool {
    let standard = standard.unwrap_or_default();
    let standard_version = standard_version.unwrap_or_default();

    (rule.core_id == "CORE-000478" && standard.eq_ignore_ascii_case("SENDIG"))
        || (rule.core_id == "CORE-000217"
            && standard.eq_ignore_ascii_case("SENDIG")
            && standard_version == "3.1")
}

#[cfg(test)]
mod tests {
    use core_rule_model::{
        Condition, ConditionGroup, Operator, OperatorOptions, RuleType, ValueExpr,
    };
    use serde_json::Value;

    use super::*;

    fn rule_with_standard(rule_id: &str, name: &str, version: &str) -> ExecutableRule {
        ExecutableRule {
            core_id: rule_id.to_owned(),
            standards: vec![StandardRef {
                name: Some(name.to_owned()),
                version: Some(version.to_owned()),
                ..Default::default()
            }],
            sensitivity: None,
            executability: None,
            description: None,
            authorities: Vec::new(),
            classes: None,
            domains: None,
            datasets: None,
            entities: None,
            rule_type: RuleType::RecordData,
            conditions: ConditionGroup::Leaf(Condition {
                target: Some("USUBJID".to_owned()),
                operator: Operator::Exists,
                comparator: ValueExpr::Literal(Value::Null),
                options: OperatorOptions::default(),
            }),
            actions: Vec::new(),
            operations: Vec::new(),
            output_variables: Vec::new(),
            grouping_variables: Vec::new(),
            use_case: None,
            status: None,
            raw: None,
            author: None,
        }
    }

    #[test]
    fn core_000642_matches_sendig_31_fixture_standard() {
        let rule = rule_with_standard("CORE-000642", "SENDIG", "3.1.1");
        assert!(rule_matches_standard(
            &rule,
            &Some("SENDIG".to_owned()),
            &Some("3.1".to_owned())
        ));
    }

    #[test]
    fn core_000478_sendig_30_remains_oracle_gap_standard_mismatch() {
        let rule = rule_with_standard("CORE-000478", "SENDIG", "3.1");
        assert!(!rule_matches_standard(
            &rule,
            &Some("SENDIG".to_owned()),
            &Some("3.0".to_owned())
        ));
        let skipped = standard_mismatch_result(&rule, Some("SENDIG"), Some("3.0"));
        assert_eq!(
            skipped.skipped_reason,
            Some(SkippedReason::OracleSemanticsGap)
        );
    }
}
