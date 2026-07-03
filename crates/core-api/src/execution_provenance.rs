use core_engine::RuleValidationResult;

use crate::rule_id_uses_hand_port;

pub(crate) fn annotate_results_execution_provenance(results: &mut [RuleValidationResult]) {
    for result in results {
        if result.execution_provenance.is_some() || result.rule_id.trim().is_empty() {
            continue;
        }
        result.execution_provenance = Some(
            if rule_id_uses_hand_port(&result.rule_id) {
                "rule_id_hand_port"
            } else {
                "native_engine"
            }
            .to_owned(),
        );
    }
}
