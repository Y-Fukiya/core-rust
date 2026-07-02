use std::collections::{BTreeMap, BTreeSet};

use crate::open_rules::discovery::OpenRulesCase;
use crate::open_rules::normalize::IssueKey;

use super::policy::{output_context_variable_oracle_gap_category, row_locator_oracle_gap_category};

pub(super) fn normalize_deferred_oracle_gap_issue_identity(
    case: &OpenRulesCase,
    official: &mut [IssueKey],
    candidate: &mut Vec<IssueKey>,
) -> Vec<String> {
    let mut normalizations = Vec::new();
    if row_locator_oracle_gap_category(case) {
        clear_issue_record_locators(official);
        clear_issue_record_locators(candidate);
        normalizations.push("row_locator_identity_relaxed".to_owned());
    }
    if output_context_variable_oracle_gap_category(case) {
        drop_candidate_output_context_variables(official, candidate);
        normalizations.push("output_context_variable_aligned".to_owned());
    }
    normalizations
}

fn clear_issue_record_locators(issues: &mut [IssueKey]) {
    for issue in issues {
        issue.row.clear();
        issue.usubjid.clear();
        issue.seq.clear();
    }
}

fn drop_candidate_output_context_variables(official: &[IssueKey], candidate: &mut Vec<IssueKey>) {
    let official_variables_by_location = official
        .iter()
        .flat_map(|issue| {
            issue.variables.iter().map(|variable| {
                (
                    (
                        issue.rule_id.clone(),
                        issue.dataset.clone(),
                        issue.domain.clone(),
                        issue.row.clone(),
                        issue.usubjid.clone(),
                        issue.seq.clone(),
                    ),
                    variable.clone(),
                )
            })
        })
        .fold(
            BTreeMap::<_, BTreeSet<String>>::new(),
            |mut variables_by_location, (location, variable)| {
                variables_by_location
                    .entry(location)
                    .or_default()
                    .insert(variable);
                variables_by_location
            },
        );

    candidate.retain(|issue| {
        let location = (
            issue.rule_id.clone(),
            issue.dataset.clone(),
            issue.domain.clone(),
            issue.row.clone(),
            issue.usubjid.clone(),
            issue.seq.clone(),
        );
        let Some(official_variables) = official_variables_by_location.get(&location) else {
            return true;
        };
        issue
            .variables
            .iter()
            .any(|variable| official_variables.contains(variable))
    });
}
