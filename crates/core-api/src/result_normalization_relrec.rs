use std::collections::BTreeSet;

use core_data::{dataset_column_values, LoadedDataset};
use core_engine::{RuleValidationResult, ValidationIssue};
use core_rule_model::ExecutableRule;
use serde_json::Value;

use crate::dataset_helpers::{dataset_column_name, dataset_domain_value, dataset_has_column};
use crate::json_values::json_scalar_string;
use crate::result_normalization::core_000677_sequence_column;
use crate::{dataset_matches_name, engine_semantics, find_dataset, outcome_message};

pub(crate) fn core_000744_relrec_faobj_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_relrec_faobj_oracle_result_rule(rule) {
        return None;
    }

    let fa = find_dataset(datasets, "FA")?;

    if !dataset_has_column(fa, "FALNKID")
        && !dataset_has_column(fa, "FALNKGRP")
        && !dataset_has_column(fa, "FASPID")
    {
        return None;
    }

    let faobj = dataset_column_values(fa, "FAOBJ").ok()?;
    let fa_usubjid = dataset_column_values(fa, "USUBJID").unwrap_or_default();
    let fa_lnkid = dataset_column_values(fa, "FALNKID").unwrap_or_default();
    let fa_lnkgrp = dataset_column_values(fa, "FALNKGRP").unwrap_or_default();
    let fa_spid = dataset_column_values(fa, "FASPID").unwrap_or_default();
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let mut errors = Vec::new();

    for row in 0..fa.summary().row_count {
        let Some(faobj) = faobj
            .get(row)
            .and_then(|value| value.as_str())
            .map(str::trim)
        else {
            continue;
        };
        if faobj.is_empty() {
            continue;
        }
        let subject = fa_usubjid
            .get(row)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default();
        let link_candidates = [
            fa_lnkid
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
            fa_lnkgrp
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
            fa_spid
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim),
        ];

        let mut related_values = BTreeSet::new();
        let mut related_domains = BTreeSet::new();
        let used_specific_relrec = collect_core_000744_relrec_parent_values(
            datasets,
            fa,
            row,
            subject,
            &mut related_values,
            &mut related_domains,
        );
        if !used_specific_relrec {
            for parent in datasets.iter().filter(|dataset| {
                !dataset_matches_name(dataset, "FA") && !dataset_matches_name(dataset, "RELREC")
            }) {
                collect_core_000744_parent_values(
                    parent,
                    subject,
                    &link_candidates,
                    &mut related_values,
                    &mut related_domains,
                );
            }
        }

        if !related_values.is_empty()
            && !related_values
                .iter()
                .any(|value| value.eq_ignore_ascii_case(faobj))
        {
            errors.push(ValidationIssue {
                rule_id: rule.core_id.clone(),
                dataset: fa.metadata.name.clone(),
                domain: fa.metadata.domain.clone(),
                row: Some(row + 1),
                variables: core_000744_issue_variables(&related_domains),
                message: message.clone(),
                usubjid: (!subject.is_empty()).then(|| subject.to_owned()),
                seq: None,
            });
        }
    }

    if !errors.is_empty() {
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: fa.metadata.name.clone(),
            domain: fa.metadata.domain.clone(),
            message,
            error_count: errors.len(),
            errors,
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: fa.metadata.name.clone(),
        domain: fa.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    })
}

fn core_000744_issue_variables(related_domains: &BTreeSet<String>) -> Vec<String> {
    let placeholder = if related_domains
        .iter()
        .any(|domain| domain.eq_ignore_ascii_case("EX"))
    {
        "__"
    } else {
        "**"
    };
    vec![
        "FAOBJ".to_owned(),
        format!("RELREC.{placeholder}TERM"),
        format!("RELREC.{placeholder}TRT"),
        format!("RELREC.{placeholder}DECOD"),
    ]
}

fn collect_core_000744_relrec_parent_values(
    datasets: &[LoadedDataset],
    fa: &LoadedDataset,
    fa_row: usize,
    subject: &str,
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) -> bool {
    let Some(relrec) = find_dataset(datasets, "RELREC") else {
        return false;
    };
    let rdomains = dataset_column_values(relrec, "RDOMAIN").unwrap_or_default();
    let usubjids = dataset_column_values(relrec, "USUBJID").unwrap_or_default();
    let idvars = dataset_column_values(relrec, "IDVAR").unwrap_or_default();
    let idvarvals = dataset_column_values(relrec, "IDVARVAL").unwrap_or_default();
    let relids = dataset_column_values(relrec, "RELID").unwrap_or_default();

    let mut has_specific_fa_links = false;
    let mut fa_relids = BTreeSet::new();
    for row in 0..relrec.summary().row_count {
        if !core_000744_cell(&rdomains, row).eq_ignore_ascii_case("FA") {
            continue;
        }
        if !subject.is_empty() {
            let relrec_subject = core_000744_cell(&usubjids, row);
            if !relrec_subject.is_empty() && relrec_subject != subject {
                continue;
            }
        }
        let idvar = core_000744_cell(&idvars, row);
        let idvarval = core_000744_cell(&idvarvals, row);
        let relid = core_000744_cell(&relids, row);
        if idvar.is_empty() || idvarval.is_empty() || relid.is_empty() {
            continue;
        }
        has_specific_fa_links = true;
        let fa_value = dataset_column_values(fa, &idvar)
            .ok()
            .and_then(|values| values.get(fa_row).and_then(json_scalar_string));
        if fa_value
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value == idvarval)
        {
            fa_relids.insert(relid);
        }
    }

    if fa_relids.is_empty() {
        return has_specific_fa_links;
    }

    for row in 0..relrec.summary().row_count {
        let relid = core_000744_cell(&relids, row);
        if !fa_relids.contains(&relid) {
            continue;
        }
        let rdomain = core_000744_cell(&rdomains, row);
        if rdomain.is_empty() || rdomain.eq_ignore_ascii_case("FA") {
            continue;
        }
        let idvar = core_000744_cell(&idvars, row);
        let idvarval = core_000744_cell(&idvarvals, row);
        if idvar.is_empty() || idvarval.is_empty() {
            continue;
        }
        let relrec_subject = core_000744_cell(&usubjids, row);
        let Some(parent) = find_dataset(datasets, &rdomain) else {
            continue;
        };
        collect_core_000744_parent_row_values(
            parent,
            &idvar,
            &idvarval,
            if relrec_subject.is_empty() {
                subject
            } else {
                &relrec_subject
            },
            related_values,
            related_domains,
        );
    }

    true
}

fn collect_core_000744_parent_row_values(
    parent: &LoadedDataset,
    idvar: &str,
    idvarval: &str,
    subject: &str,
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) {
    let Ok(id_values) = dataset_column_values(parent, idvar) else {
        return;
    };
    let subject_values = dataset_column_values(parent, "USUBJID").unwrap_or_default();
    let domain = dataset_domain_value(parent);
    for row in 0..parent.summary().row_count {
        if core_000744_cell(&id_values, row) != idvarval {
            continue;
        }
        if !subject.is_empty() {
            let parent_subject = core_000744_cell(&subject_values, row);
            if !parent_subject.is_empty() && parent_subject != subject {
                continue;
            }
        }
        related_domains.insert(domain.clone());
        collect_core_000744_parent_row_term_values(parent, row, &domain, related_values);
    }
}

fn collect_core_000744_parent_values(
    dataset: &LoadedDataset,
    subject: &str,
    link_candidates: &[Option<&str>; 3],
    related_values: &mut BTreeSet<String>,
    related_domains: &mut BTreeSet<String>,
) {
    let domain = dataset_domain_value(dataset);
    let subject_values = dataset_column_values(dataset, "USUBJID").unwrap_or_default();
    let link_columns = [
        format!("{domain}LNKID"),
        format!("{domain}LNKGRP"),
        format!("{domain}SPID"),
        format!("{domain}SEQ"),
    ];
    let link_values = link_columns
        .iter()
        .map(|column| dataset_column_values(dataset, column).unwrap_or_default())
        .collect::<Vec<_>>();

    for row in 0..dataset.summary().row_count {
        if !subject.is_empty() {
            let parent_subject = subject_values
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            if !parent_subject.is_empty() && parent_subject != subject {
                continue;
            }
        }

        let linked = link_values.iter().any(|values| {
            let parent_link = values
                .get(row)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            !parent_link.is_empty()
                && link_candidates
                    .iter()
                    .flatten()
                    .any(|candidate| !candidate.is_empty() && *candidate == parent_link)
        });
        if !linked {
            continue;
        }

        related_domains.insert(domain.clone());
        collect_core_000744_parent_row_term_values(dataset, row, &domain, related_values);
    }
}

fn collect_core_000744_parent_row_term_values(
    dataset: &LoadedDataset,
    row: usize,
    domain: &str,
    related_values: &mut BTreeSet<String>,
) {
    for column in [
        format!("{domain}TERM"),
        format!("{domain}TRT"),
        format!("{domain}DECOD"),
    ] {
        if let Ok(values) = dataset_column_values(dataset, &column) {
            if let Some(value) = values.get(row).and_then(json_scalar_string) {
                let value = value.trim();
                if !value.is_empty() {
                    related_values.insert(value.to_owned());
                }
            }
        }
    }
}

fn core_000744_cell(values: &[Value], row: usize) -> String {
    values
        .get(row)
        .and_then(json_scalar_string)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct Core000757RelrecEndpoint {
    domain: String,
    idvar: String,
    idvarval: Option<String>,
    relid: String,
    subject: Option<String>,
}

pub(crate) fn core_000757_intervention_relrec_faobj_result(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<RuleValidationResult> {
    if !engine_semantics::is_intervention_relrec_faobj_rule(rule) {
        return None;
    }

    let result_dataset = datasets
        .iter()
        .find(|dataset| !dataset_matches_name(dataset, "RELREC"))
        .or_else(|| datasets.first())?;
    let message = outcome_message(rule).unwrap_or_else(|| format!("Rule {} failed", rule.core_id));
    let Some(fa) = find_dataset(datasets, "FA") else {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    };
    let Some(relrec) = find_dataset(datasets, "RELREC") else {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    };
    let endpoints = core_000757_relrec_endpoints(relrec);
    let fa_endpoints = endpoints
        .iter()
        .filter(|endpoint| endpoint.domain.eq_ignore_ascii_case("FA"))
        .collect::<Vec<_>>();
    if fa_endpoints.is_empty() {
        return Some(core_000757_passed_result(rule, result_dataset, message));
    }

    let faobj_values = dataset_column_values(fa, "FAOBJ").ok()?;
    let fa_subjects = dataset_column_values(fa, "USUBJID").unwrap_or_default();
    let mut errors = Vec::new();
    let mut reported_rows = BTreeSet::<(String, usize)>::new();

    for parent in datasets.iter().filter(|dataset| {
        !dataset_matches_name(dataset, "FA") && !dataset_matches_name(dataset, "RELREC")
    }) {
        let domain = dataset_domain_value(parent);
        let parent_endpoints = endpoints
            .iter()
            .filter(|endpoint| endpoint.domain.eq_ignore_ascii_case(&domain))
            .collect::<Vec<_>>();
        if parent_endpoints.is_empty() {
            continue;
        }
        let Some(trt_column) = dataset_column_name(parent, &format!("{domain}TRT")) else {
            continue;
        };
        let Some(decod_column) = dataset_column_name(parent, &format!("{domain}DECOD")) else {
            continue;
        };
        let Ok(trt_values) = dataset_column_values(parent, &trt_column) else {
            continue;
        };
        let Ok(decod_values) = dataset_column_values(parent, &decod_column) else {
            continue;
        };
        let parent_subjects = dataset_column_values(parent, "USUBJID").unwrap_or_default();
        let seq_column = core_000677_sequence_column(parent);
        let seq_values = seq_column
            .as_deref()
            .and_then(|column| dataset_column_values(parent, column).ok())
            .unwrap_or_default();

        for row in 0..parent.summary().row_count {
            if !core_000744_cell(&decod_values, row).is_empty() {
                continue;
            }
            let trt = core_000744_cell(&trt_values, row);
            let parent_subject = core_000744_cell(&parent_subjects, row);
            let mut has_mismatch = false;

            for parent_endpoint in &parent_endpoints {
                let Some(parent_key) = core_000757_endpoint_row_key(parent, parent_endpoint, row)
                else {
                    continue;
                };
                if !core_000757_endpoint_subject_allows(parent_endpoint, &parent_subject) {
                    continue;
                }
                for fa_endpoint in fa_endpoints
                    .iter()
                    .filter(|fa_endpoint| fa_endpoint.relid == parent_endpoint.relid)
                {
                    for fa_row in 0..fa.summary().row_count {
                        let Some(fa_key) = core_000757_endpoint_row_key(fa, fa_endpoint, fa_row)
                        else {
                            continue;
                        };
                        if !core_000757_relrec_keys_match(
                            parent_endpoint,
                            &parent_key,
                            fa_endpoint,
                            &fa_key,
                        ) {
                            continue;
                        }
                        let fa_subject = core_000744_cell(&fa_subjects, fa_row);
                        if !core_000757_endpoint_subject_allows(fa_endpoint, &fa_subject)
                            || !core_000757_subjects_match(&parent_subject, &fa_subject)
                        {
                            continue;
                        }
                        let faobj = core_000744_cell(&faobj_values, fa_row);
                        if !faobj.is_empty() && trt != faobj {
                            has_mismatch = true;
                            break;
                        }
                    }
                    if has_mismatch {
                        break;
                    }
                }
                if has_mismatch {
                    break;
                }
            }

            if has_mismatch && reported_rows.insert((parent.metadata.name.clone(), row)) {
                let usubjid = (!parent_subject.is_empty()).then(|| parent_subject.clone());
                let seq = core_000744_cell(&seq_values, row);
                errors.push(ValidationIssue {
                    rule_id: rule.core_id.clone(),
                    dataset: parent.metadata.name.clone(),
                    domain: parent.metadata.domain.clone(),
                    row: Some(row + 1),
                    variables: vec![
                        trt_column.clone(),
                        decod_column.clone(),
                        "RELREC.FAOBJ".to_owned(),
                    ],
                    message: message.clone(),
                    usubjid,
                    seq: (!seq.is_empty()).then_some(seq),
                });
            }
        }
    }

    if !errors.is_empty() {
        return Some(RuleValidationResult {
            rule_id: rule.core_id.clone(),
            execution_status: core_engine::ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: errors
                .first()
                .map(|issue| issue.dataset.clone())
                .unwrap_or_else(|| fa.metadata.name.clone()),
            domain: errors
                .first()
                .and_then(|issue| issue.domain.clone())
                .or_else(|| fa.metadata.domain.clone()),
            message,
            error_count: errors.len(),
            errors,
        });
    }

    Some(RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: fa.metadata.name.clone(),
        domain: fa.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    })
}

fn core_000757_passed_result(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    message: String,
) -> RuleValidationResult {
    RuleValidationResult {
        rule_id: rule.core_id.clone(),
        execution_status: core_engine::ExecutionStatus::Passed,
        execution_provenance: None,
        skipped_reason: None,
        dataset: dataset.metadata.name.clone(),
        domain: dataset.metadata.domain.clone(),
        message,
        error_count: 0,
        errors: Vec::new(),
    }
}

fn core_000757_relrec_endpoints(relrec: &LoadedDataset) -> Vec<Core000757RelrecEndpoint> {
    let rdomains = dataset_column_values(relrec, "RDOMAIN").unwrap_or_default();
    let subjects = dataset_column_values(relrec, "USUBJID").unwrap_or_default();
    let idvars = dataset_column_values(relrec, "IDVAR").unwrap_or_default();
    let idvarvals = dataset_column_values(relrec, "IDVARVAL").unwrap_or_default();
    let relids = dataset_column_values(relrec, "RELID").unwrap_or_default();

    (0..relrec.summary().row_count)
        .filter_map(|row| {
            let domain = core_000744_cell(&rdomains, row);
            let idvar = core_000744_cell(&idvars, row);
            let relid = core_000744_cell(&relids, row);
            if domain.is_empty() || idvar.is_empty() || relid.is_empty() {
                return None;
            }
            let subject = core_000744_cell(&subjects, row);
            let idvarval = core_000744_cell(&idvarvals, row);
            Some(Core000757RelrecEndpoint {
                domain: domain.to_ascii_uppercase(),
                idvar,
                idvarval: (!idvarval.is_empty()).then_some(idvarval),
                relid,
                subject: (!subject.is_empty()).then_some(subject),
            })
        })
        .collect()
}

fn core_000757_endpoint_row_key(
    dataset: &LoadedDataset,
    endpoint: &Core000757RelrecEndpoint,
    row: usize,
) -> Option<String> {
    let values = dataset_column_values(dataset, &endpoint.idvar).ok()?;
    let value = core_000744_cell(&values, row);
    if value.is_empty() {
        return None;
    }
    match &endpoint.idvarval {
        Some(expected) if value == *expected => Some(value),
        Some(_) => None,
        None => Some(value),
    }
}

fn core_000757_relrec_keys_match(
    parent_endpoint: &Core000757RelrecEndpoint,
    parent_key: &str,
    fa_endpoint: &Core000757RelrecEndpoint,
    fa_key: &str,
) -> bool {
    match (&parent_endpoint.idvarval, &fa_endpoint.idvarval) {
        (Some(_), Some(_)) => true,
        (None, None) => parent_key == fa_key,
        _ => false,
    }
}

fn core_000757_endpoint_subject_allows(
    endpoint: &Core000757RelrecEndpoint,
    row_subject: &str,
) -> bool {
    endpoint
        .subject
        .as_deref()
        .is_none_or(|subject| row_subject.is_empty() || subject == row_subject)
}

fn core_000757_subjects_match(left: &str, right: &str) -> bool {
    left.is_empty() || right.is_empty() || left == right
}
