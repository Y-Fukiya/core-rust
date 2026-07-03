use std::collections::{BTreeMap, BTreeSet};

use core_data::{dataset_column_values, LoadedDataset};
use core_engine::{RuleValidationResult, ValidationIssue};
use core_rule_model::ExecutableRule;
use serde_json::Value;

use crate::json_values::json_report_string;
use crate::scope_filter::{
    class_scope_matches, domain_class_name, scope_contains_all, scope_matches, scope_values,
};
use crate::{
    dataset_column_name, dataset_domain_value, engine_semantics, outcome_message,
    push_unique_string,
};

#[derive(Debug, Clone)]
struct SplitDomainUniqueSetRow {
    dataset: String,
    domain: String,
    row: usize,
    usubjid: Option<String>,
    poolid: Option<String>,
    seq_column: String,
    seq: String,
}

pub(crate) fn core_000750_split_domain_unique_set_results(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    existing_results: &[RuleValidationResult],
) -> Vec<RuleValidationResult> {
    if rule.core_id != engine_semantics::CORE_000750 {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for dataset in datasets
        .iter()
        .filter(|dataset| core_000750_split_domain_dataset_allowed(rule, dataset))
    {
        let Some(seq_column) = split_domain_sequence_column(dataset) else {
            continue;
        };
        let Some(seq_values) = resolved_dataset_column_values(dataset, &seq_column) else {
            continue;
        };
        let domain_values = resolved_dataset_column_values(dataset, "DOMAIN").unwrap_or_default();
        let usubjid_values = resolved_dataset_column_values(dataset, "USUBJID").unwrap_or_default();
        let poolid_values = resolved_dataset_column_values(dataset, "POOLID").unwrap_or_default();
        let dataset_name = dataset.metadata.name.to_ascii_uppercase();
        for row in 0..dataset.frame().height() {
            let seq = json_report_string(seq_values.get(row).unwrap_or(&Value::Null));
            if seq.trim().is_empty() {
                continue;
            }
            let domain = domain_values
                .get(row)
                .map(json_report_string)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| dataset_domain_value(dataset));
            let usubjid = usubjid_values
                .get(row)
                .map(json_report_string)
                .filter(|value| !value.trim().is_empty());
            let poolid = poolid_values
                .get(row)
                .map(json_report_string)
                .filter(|value| !value.trim().is_empty());
            rows.push(SplitDomainUniqueSetRow {
                dataset: dataset_name.clone(),
                domain: domain.to_ascii_uppercase(),
                row: row + 1,
                usubjid,
                poolid,
                seq_column: seq_column.clone(),
                seq,
            });
        }
    }

    let mut key_datasets = BTreeMap::<Vec<String>, BTreeSet<String>>::new();
    for row in &rows {
        for key in split_domain_unique_set_keys(row) {
            key_datasets
                .entry(key)
                .or_default()
                .insert(row.dataset.clone());
        }
    }

    let existing = existing_results
        .iter()
        .flat_map(|result| result.errors.iter())
        .filter_map(|issue| {
            Some((
                issue.dataset.to_ascii_uppercase(),
                issue.row?,
                issue.seq.clone().unwrap_or_default(),
            ))
        })
        .collect::<BTreeSet<_>>();
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let mut issues_by_dataset = BTreeMap::<String, Vec<ValidationIssue>>::new();
    let mut seen_rows = BTreeSet::<(String, usize, String)>::new();
    for row in rows {
        let crosses_split_dataset = split_domain_unique_set_keys(&row).into_iter().any(|key| {
            key_datasets
                .get(&key)
                .is_some_and(|datasets| datasets.len() > 1)
        });
        if !crosses_split_dataset {
            continue;
        }
        let issue_key = (row.dataset.clone(), row.row, row.seq.clone());
        if existing.contains(&issue_key) || !seen_rows.insert(issue_key) {
            continue;
        }
        let mut variables = vec![row.seq_column.clone()];
        if row.usubjid.is_some() {
            variables.insert(0, "USUBJID".to_owned());
        }
        push_unique_string(&mut variables, "POOLID");
        let issue = ValidationIssue {
            rule_id: rule.core_id.clone(),
            dataset: row.dataset.clone(),
            domain: Some(row.domain),
            row: Some(row.row),
            variables,
            message: message.clone(),
            usubjid: row.usubjid,
            seq: Some(row.seq),
        };
        issues_by_dataset
            .entry(issue.dataset.clone())
            .or_default()
            .push(issue);
    }

    issues_by_dataset
        .into_iter()
        .map(|(dataset, errors)| RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset,
            domain: errors.first().and_then(|issue| issue.domain.clone()),
            message: message.clone(),
            error_count: errors.len(),
            errors,
        })
        .collect()
}

fn core_000750_split_domain_dataset_allowed(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> bool {
    let row_domain = dataset_domain_value(dataset);
    let domains = [
        dataset.metadata.name.as_str(),
        dataset
            .metadata
            .domain
            .as_deref()
            .unwrap_or(&dataset.metadata.name),
        row_domain.as_str(),
    ];

    if !core_000750_split_domain_class_scope_allows(rule.classes.as_ref(), &domains) {
        return false;
    }

    let includes = scope_values(rule.domains.as_ref(), "Include");
    let excludes = scope_values(rule.domains.as_ref(), "Exclude");

    if domains
        .iter()
        .any(|domain| scope_matches(&excludes, domain))
    {
        return false;
    }

    includes.is_empty()
        || scope_contains_all(&includes)
        || domains
            .iter()
            .any(|domain| scope_matches(&includes, domain))
}

fn core_000750_split_domain_class_scope_allows(scope: Option<&Value>, domains: &[&str]) -> bool {
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");
    let classes = domains
        .iter()
        .filter_map(|domain| domain_class_name(domain))
        .collect::<Vec<_>>();

    if classes.is_empty() {
        return true;
    }
    if classes
        .iter()
        .any(|class| class_scope_matches(&excludes, class))
    {
        return false;
    }
    includes.is_empty()
        || scope_contains_all(&includes)
        || classes
            .iter()
            .any(|class| class_scope_matches(&includes, class))
}

fn split_domain_unique_set_keys(row: &SplitDomainUniqueSetRow) -> Vec<Vec<String>> {
    let mut keys = Vec::new();
    if let Some(usubjid) = &row.usubjid {
        keys.push(vec![
            "USUBJID".to_owned(),
            row.domain.clone(),
            usubjid.clone(),
            row.seq.clone(),
        ]);
    }
    if let Some(poolid) = &row.poolid {
        keys.push(vec![
            "POOLID".to_owned(),
            row.domain.clone(),
            poolid.clone(),
            row.seq.clone(),
        ]);
    }
    keys
}

fn split_domain_sequence_column(dataset: &LoadedDataset) -> Option<String> {
    let domain = dataset_domain_value(dataset);
    dataset_column_name(dataset, &format!("{domain}SEQ")).or_else(|| {
        dataset
            .summary()
            .columns
            .into_iter()
            .find(|column| column.to_ascii_uppercase().ends_with("SEQ"))
    })
}

fn resolved_dataset_column_values(dataset: &LoadedDataset, column: &str) -> Option<Vec<Value>> {
    let actual = dataset_column_name(dataset, column)?;
    dataset_column_values(dataset, &actual).ok()
}
