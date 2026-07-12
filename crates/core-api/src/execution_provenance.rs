use core_engine::RuleValidationResult;

use crate::{rule_id_specific_semantics_classification, rule_id_uses_hand_port};

pub(crate) fn annotate_results_execution_provenance(results: &mut [RuleValidationResult]) {
    for result in results {
        if result.execution_provenance.is_some() || result.rule_id.trim().is_empty() {
            continue;
        }
        result.execution_provenance =
            Some(execution_provenance_for_rule_id(&result.rule_id).to_owned());
    }
}

fn execution_provenance_for_rule_id(rule_id: &str) -> &'static str {
    if rule_id_uses_hand_port(rule_id) {
        return "rule_id_hand_port";
    }
    match rule_id_specific_semantics_classification(rule_id) {
        Some("engine_semantics") => "rule_specific_engine_semantics",
        Some("compatibility_policy") => "compatibility_policy",
        Some("hand_port_semantics") => "rule_id_hand_port",
        Some(_) | None => "generic_engine",
    }
}

#[cfg(test)]
mod tests {
    use super::execution_provenance_for_rule_id;

    #[test]
    fn production_provenance_distinguishes_execution_paths() {
        assert_eq!(
            execution_provenance_for_rule_id("CORE-999999"),
            "generic_engine"
        );
        assert_eq!(
            execution_provenance_for_rule_id("CORE-000007"),
            "rule_specific_engine_semantics"
        );
        assert_eq!(
            execution_provenance_for_rule_id("CORE-000119"),
            "compatibility_policy"
        );
        assert_eq!(
            execution_provenance_for_rule_id("CORE-000583"),
            "rule_id_hand_port"
        );
    }
}
