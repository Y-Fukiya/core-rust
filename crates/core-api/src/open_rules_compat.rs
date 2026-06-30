//! Open Rules oracle-compatibility helpers.
//!
//! This module is intentionally separate from the generic validation path. Open
//! Rules compatibility gaps are coverage decisions for the oracle harness, not
//! production engine semantics.

use core_engine::RuleValidationResult;
use core_rule_model::ExecutableRule;

pub(crate) fn post_execution_oracle_gap_result(
    rule: &ExecutableRule,
    result: &RuleValidationResult,
) -> Option<RuleValidationResult> {
    let _ = (rule, result);
    // Do not rewrite executed engine output into skipped oracle-gap rows. Keeping
    // failures as failures preserves scoreboard independence.
    None
}
