use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::open_rules::discovery::OpenRulesCase;
use crate::open_rules::normalize::{normalize_scalar, IssueKey};

type IssueSignature = (String, String, String, Vec<String>);
type IssueRowSignature = (String, String, String, Vec<String>, String);

pub(super) fn align_candidate_identity_to_official(
    official: &[IssueKey],
    candidate: Vec<IssueKey>,
    duplicate_sequence_values: &BTreeSet<(String, String)>,
) -> Vec<IssueKey> {
    let compare_usubjid = official.iter().any(|issue| !issue.usubjid.is_empty());
    let compare_seq = official.iter().any(|issue| !issue.seq.is_empty());
    let unlocated_rules = official
        .iter()
        .filter(|issue| issue_is_unlocated(issue))
        .map(|issue| issue.rule_id.clone())
        .collect::<BTreeSet<_>>();
    let official_dataset_issues = official
        .iter()
        .filter(|issue| issue.row.is_empty() && issue.usubjid.is_empty() && issue.seq.is_empty())
        .map(issue_signature)
        .collect::<BTreeSet<_>>();
    let official_row_issues = official
        .iter()
        .filter(|issue| !issue.row.is_empty())
        .map(issue_row_signature)
        .collect::<BTreeSet<_>>();
    let candidate_seq_rows = candidate_seq_row_index(&candidate);

    candidate
        .into_iter()
        .map(|mut issue| {
            if unlocated_rules.contains(&issue.rule_id) {
                issue.dataset.clear();
                issue.domain.clear();
                issue.row.clear();
                issue.variables.clear();
                issue.usubjid.clear();
                issue.seq.clear();
            } else if official_dataset_issues.contains(&issue_signature(&issue)) {
                issue.row.clear();
                issue.usubjid.clear();
                issue.seq.clear();
            } else {
                if !compare_seq
                    && candidate_seq_matches_official_row(
                        &issue,
                        &official_row_issues,
                        &candidate_seq_rows,
                        duplicate_sequence_values,
                    )
                {
                    issue.row.clone_from(&issue.seq);
                }
                if !compare_usubjid {
                    issue.usubjid.clear();
                }
            }
            if !compare_seq {
                issue.seq.clear();
            }
            issue
        })
        .collect()
}

fn issue_is_unlocated(issue: &IssueKey) -> bool {
    issue.dataset.is_empty()
        && issue.domain.is_empty()
        && issue.row.is_empty()
        && issue.variables.is_empty()
        && issue.usubjid.is_empty()
        && issue.seq.is_empty()
}

fn issue_signature(issue: &IssueKey) -> IssueSignature {
    (
        issue.rule_id.clone(),
        issue.dataset.clone(),
        issue.domain.clone(),
        issue.variables.clone(),
    )
}

fn candidate_seq_matches_official_row(
    issue: &IssueKey,
    official_row_issues: &BTreeSet<IssueRowSignature>,
    candidate_seq_rows: &BTreeMap<IssueRowSignature, BTreeSet<String>>,
    duplicate_sequence_values: &BTreeSet<(String, String)>,
) -> bool {
    let seq_signature = (
        issue.rule_id.clone(),
        issue.dataset.clone(),
        issue.domain.clone(),
        issue.variables.clone(),
        issue.seq.clone(),
    );
    !issue.seq.is_empty()
        && issue.row != issue.seq
        && !official_row_issues.contains(&issue_row_signature(issue))
        && duplicate_sequence_values.contains(&(issue.dataset.clone(), issue.seq.clone()))
        && official_row_issues.contains(&seq_signature)
        && candidate_seq_rows
            .get(&seq_signature)
            .is_some_and(|rows| rows.len() == 1 && rows.contains(&issue.row))
}

pub(super) fn duplicate_sequence_values_by_dataset(
    case: &OpenRulesCase,
) -> BTreeSet<(String, String)> {
    case.dataset_files
        .iter()
        .filter_map(|path| duplicate_sequence_values_in_dataset(path).ok())
        .flatten()
        .collect()
}

fn duplicate_sequence_values_in_dataset(path: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let dataset = path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(normalize_dataset_name)
        .unwrap_or_default();
    if dataset.is_empty() {
        return Ok(Vec::new());
    }

    let mut reader = csv::ReaderBuilder::new().flexible(true).from_path(path)?;
    let headers = reader.headers()?.clone();
    let Some(seq_index) = sequence_column_index(&headers, &dataset) else {
        return Ok(Vec::new());
    };

    let mut counts = BTreeMap::<String, usize>::new();
    for record in reader.records() {
        let record = record?;
        let value = record
            .get(seq_index)
            .map(normalize_scalar)
            .unwrap_or_default();
        if !value.is_empty() {
            *counts.entry(value).or_default() += 1;
        }
    }

    Ok(counts
        .into_iter()
        .filter_map(|(seq, count)| (count > 1).then(|| (dataset.clone(), seq)))
        .collect())
}

fn sequence_column_index(headers: &csv::StringRecord, dataset: &str) -> Option<usize> {
    let expected = format!("{dataset}SEQ");
    headers
        .iter()
        .position(|header| header.trim().eq_ignore_ascii_case(&expected))
        .or_else(|| {
            headers.iter().position(|header| {
                let header = header.trim();
                header.len() > 3
                    && header.to_ascii_uppercase().ends_with("SEQ")
                    && !header.eq_ignore_ascii_case("USUBJID")
            })
        })
}

fn normalize_dataset_name(value: &str) -> String {
    value
        .trim()
        .strip_suffix(".csv")
        .or_else(|| value.trim().strip_suffix(".CSV"))
        .unwrap_or_else(|| value.trim())
        .to_ascii_uppercase()
}

fn candidate_seq_row_index(issues: &[IssueKey]) -> BTreeMap<IssueRowSignature, BTreeSet<String>> {
    let mut index = BTreeMap::<IssueRowSignature, BTreeSet<String>>::new();
    for issue in issues {
        if issue.seq.is_empty() {
            continue;
        }
        index
            .entry((
                issue.rule_id.clone(),
                issue.dataset.clone(),
                issue.domain.clone(),
                issue.variables.clone(),
                issue.seq.clone(),
            ))
            .or_default()
            .insert(issue.row.clone());
    }
    index
}

fn issue_row_signature(issue: &IssueKey) -> IssueRowSignature {
    (
        issue.rule_id.clone(),
        issue.dataset.clone(),
        issue.domain.clone(),
        issue.variables.clone(),
        issue.row.clone(),
    )
}
