use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::OpenRulesCase;
use crate::open_rules::normalize::{normalize_scalar, IssueKey};

type IssueSignature = (String, String, String, Vec<String>);
type IssueRowSignature = (String, String, String, Vec<String>, String);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityComparison {
    RawStructural,
    Structural,
    CountOnlyUnlocated,
    MixedLocatedAndUnlocated,
}

impl IdentityComparison {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawStructural => "raw_structural",
            Self::Structural => "structural",
            Self::CountOnlyUnlocated => "count_only_unlocated",
            Self::MixedLocatedAndUnlocated => "mixed_located_and_unlocated",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityAlignmentTrace {
    pub comparison: IdentityComparison,
    pub official_unlocated_findings: usize,
    pub official_findings_with_usubjid: usize,
    pub official_findings_with_seq: usize,
    pub candidate_unlocated_rule_collapses: usize,
    pub candidate_dataset_locator_clears: usize,
    pub candidate_usubjid_omissions: usize,
    pub candidate_seq_omissions: usize,
    pub candidate_rows_substituted_from_seq: usize,
}

impl IdentityAlignmentTrace {
    pub(super) fn raw(official: &[IssueKey]) -> Self {
        Self {
            comparison: IdentityComparison::RawStructural,
            official_unlocated_findings: official
                .iter()
                .filter(|issue| issue_is_unlocated(issue))
                .count(),
            official_findings_with_usubjid: official
                .iter()
                .filter(|issue| !issue.usubjid.is_empty())
                .count(),
            official_findings_with_seq: official
                .iter()
                .filter(|issue| !issue.seq.is_empty())
                .count(),
            candidate_unlocated_rule_collapses: 0,
            candidate_dataset_locator_clears: 0,
            candidate_usubjid_omissions: 0,
            candidate_seq_omissions: 0,
            candidate_rows_substituted_from_seq: 0,
        }
    }

    pub(crate) fn case_metrics(self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.comparison.as_str()).chain(
            [
                (
                    "unlocated_rule_collapse",
                    self.candidate_unlocated_rule_collapses,
                ),
                (
                    "dataset_locator_clear",
                    self.candidate_dataset_locator_clears,
                ),
                (
                    "candidate_usubjid_omitted",
                    self.candidate_usubjid_omissions,
                ),
                ("candidate_seq_omitted", self.candidate_seq_omissions),
                (
                    "row_substituted_from_seq",
                    self.candidate_rows_substituted_from_seq,
                ),
            ]
            .into_iter()
            .filter_map(|(metric, findings)| (findings != 0).then_some(metric)),
        )
    }
}

pub(super) fn align_candidate_identity_to_official(
    official: &[IssueKey],
    candidate: Vec<IssueKey>,
    duplicate_sequence_values: &BTreeSet<(String, String)>,
) -> (Vec<IssueKey>, IdentityAlignmentTrace) {
    let mut trace = IdentityAlignmentTrace::raw(official);
    trace.comparison = match trace.official_unlocated_findings {
        0 => IdentityComparison::Structural,
        count if count == official.len() => IdentityComparison::CountOnlyUnlocated,
        _ => IdentityComparison::MixedLocatedAndUnlocated,
    };
    let compare_usubjid = trace.official_findings_with_usubjid != 0;
    let compare_seq = trace.official_findings_with_seq != 0;
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

    let aligned = candidate
        .into_iter()
        .map(|mut issue| {
            let had_usubjid = !issue.usubjid.is_empty();
            let had_seq = !issue.seq.is_empty();
            if unlocated_rules.contains(&issue.rule_id) {
                trace.candidate_unlocated_rule_collapses +=
                    usize::from(!issue_is_unlocated(&issue));
                issue.dataset.clear();
                issue.domain.clear();
                issue.row.clear();
                issue.variables.clear();
                issue.usubjid.clear();
                issue.seq.clear();
            } else if official_dataset_issues.contains(&issue_signature(&issue)) {
                trace.candidate_dataset_locator_clears +=
                    usize::from(!issue.row.is_empty() || had_usubjid || had_seq);
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
                    trace.candidate_rows_substituted_from_seq += 1;
                }
                if !compare_usubjid {
                    issue.usubjid.clear();
                }
            }
            if !compare_seq {
                issue.seq.clear();
            }
            trace.candidate_usubjid_omissions +=
                usize::from(had_usubjid && issue.usubjid.is_empty());
            trace.candidate_seq_omissions += usize::from(had_seq && issue.seq.is_empty());
            issue
        })
        .collect();
    (aligned, trace)
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
