use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::normalize::{normalize_csv, IssueKey, ReportSource};
use crate::open_rules::report::write_scoreboard;
use crate::open_rules::upstream::{load_upstream_info, UpstreamInfo};

mod identity;
mod normalization;
mod policy;
mod provenance;
mod summary;
use identity::{align_candidate_identity_to_official, duplicate_sequence_values_by_dataset};
use normalization::normalize_deferred_oracle_gap_issue_identity;
use policy::{
    deferred_default_engine_oracle_gap_reason, deferred_default_engine_oracle_gap_skipped_reason,
    official_oracle_fixture_gap_category,
};
pub use provenance::{
    execution_provenance_detail_for_case, execution_provenance_for_rule_id,
    scoring_policy_for_normalizations, ExecutionProvenance, ExecutionProvenanceDetail,
    ScoringPolicy,
};
#[cfg(test)]
pub use summary::ScoringNormalizationSummary;
pub use summary::{GroupSummary, ScoreGate, ScoreSummary};

#[derive(Debug, Clone, Parser)]
pub struct ScoreArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,

    #[arg(long, value_name = "RATIO", value_parser = parse_coverage_ratio)]
    pub min_coverage: Option<f64>,

    #[arg(long, value_name = "COUNT")]
    pub max_skipped_unsupported: Option<usize>,

    #[arg(long)]
    pub fail_on_deferred_oracle_gap: bool,

    /// Disable oracle-gap reclassification and oracle-informed score normalizations.
    ///
    /// This reports raw official-vs-candidate structural mismatches as supported
    /// mismatches, making it suitable for auditing how much the compatibility
    /// scorer changes the headline metrics.
    #[arg(long)]
    pub strict_scoring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBucket {
    SupportedMatch,
    SupportedMismatch,
    DeferredOracleGapMismatch,
    DeferredOracleGapSkipped,
    SkippedUnsupported,
    MixedSkippedAndIssues,
    NoOfficialOracle,
    HarnessError,
}

impl ScoreBucket {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SupportedMatch => "supported_match",
            Self::SupportedMismatch => "supported_mismatch",
            Self::DeferredOracleGapMismatch => "deferred_oracle_gap_mismatch",
            Self::DeferredOracleGapSkipped => "deferred_oracle_gap_skipped",
            Self::SkippedUnsupported => "skipped_unsupported",
            Self::MixedSkippedAndIssues => "mixed_skipped_and_issues",
            Self::NoOfficialOracle => "no_official_oracle",
            Self::HarnessError => "harness_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoredCase {
    pub scope: String,
    pub rule_id: String,
    pub case_kind: String,
    pub case_id: String,
    pub case_dir: PathBuf,
    pub official_results_csv: PathBuf,
    pub candidate_report_csv: PathBuf,
    #[serde(default)]
    pub execution_provenance: ExecutionProvenance,
    #[serde(default)]
    pub execution_provenance_detail: ExecutionProvenanceDetail,
    #[serde(default)]
    pub scoring_policy: ScoringPolicy,
    pub bucket: ScoreBucket,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scoring_normalizations: Vec<String>,
    pub official_issue_count: Option<usize>,
    pub candidate_issue_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_fingerprint_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<IssueKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<IssueKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scoreboard {
    pub upstream: UpstreamInfo,
    pub summary: ScoreSummary,
    #[serde(default)]
    pub gate: ScoreGate,
    pub by_scope: Vec<GroupSummary>,
    pub by_case_kind: Vec<GroupSummary>,
    pub cases: Vec<ScoredCase>,
}

pub fn run(args: ScoreArgs) -> anyhow::Result<bool> {
    let cases = discover_cases(&args.open_rules_root, &args.scope)?;
    let scored = score_cases_with_options(
        &cases,
        &args.core_rs_results_root,
        ScoreOptions {
            strict_scoring: args.strict_scoring,
        },
    );
    let upstream = load_upstream_info(&args.open_rules_root)?;
    let scoreboard = Scoreboard::new_with_gate(
        upstream,
        scored,
        args.min_coverage,
        args.max_skipped_unsupported,
        args.fail_on_deferred_oracle_gap,
    );
    write_scoreboard(&args.out, &scoreboard)?;
    Ok(scoreboard.gate.should_fail)
}

#[cfg(test)]
pub fn score_cases(cases: &[OpenRulesCase], core_rs_results_root: &Path) -> Vec<ScoredCase> {
    score_cases_with_options(cases, core_rs_results_root, ScoreOptions::default())
}

#[cfg(test)]
fn score_cases_strict(cases: &[OpenRulesCase], core_rs_results_root: &Path) -> Vec<ScoredCase> {
    score_cases_with_options(
        cases,
        core_rs_results_root,
        ScoreOptions {
            strict_scoring: true,
        },
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct ScoreOptions {
    strict_scoring: bool,
}

fn score_cases_with_options(
    cases: &[OpenRulesCase],
    core_rs_results_root: &Path,
    options: ScoreOptions,
) -> Vec<ScoredCase> {
    cases
        .iter()
        .map(|case| score_case(case, core_rs_results_root, options))
        .collect()
}

pub fn relative_candidate_report_path(case: &OpenRulesCase) -> PathBuf {
    Path::new(&case.scope)
        .join(&case.rule_id)
        .join(case.case_kind.as_str())
        .join(&case.case_id)
        .join("report.csv")
}

fn score_case(
    case: &OpenRulesCase,
    core_rs_results_root: &Path,
    options: ScoreOptions,
) -> ScoredCase {
    let candidate_report_csv = core_rs_results_root.join(relative_candidate_report_path(case));
    let execution_provenance =
        provenance::candidate_execution_provenance(case, &candidate_report_csv);
    let base = ScoredCase {
        scope: case.scope.clone(),
        rule_id: case.rule_id.clone(),
        case_kind: case.case_kind.as_str().to_owned(),
        case_id: case.case_id.clone(),
        case_dir: case.case_dir.clone(),
        official_results_csv: case.official_results_csv.clone(),
        candidate_report_csv: candidate_report_csv.clone(),
        execution_provenance: execution_provenance.clone(),
        execution_provenance_detail: provenance::execution_provenance_detail_for_case(
            &case.rule_id,
            &execution_provenance,
            &[],
        ),
        scoring_policy: ScoringPolicy::StrictIdentity,
        bucket: ScoreBucket::HarnessError,
        reason: None,
        skipped_reasons: Vec::new(),
        scoring_normalizations: Vec::new(),
        official_issue_count: None,
        candidate_issue_count: None,
        missing_count: None,
        extra_count: None,
        issue_fingerprint_hash: None,
        missing: Vec::new(),
        extra: Vec::new(),
    };

    if !case.official_results_csv.is_file() {
        return ScoredCase {
            bucket: ScoreBucket::NoOfficialOracle,
            reason: missing_official_reason(case, &candidate_report_csv),
            ..base
        };
    }
    if !candidate_report_csv.is_file() {
        return ScoredCase {
            reason: Some("missing candidate report.csv".to_owned()),
            ..base
        };
    }

    let official = match normalize_csv(
        &case.official_results_csv,
        ReportSource::Official,
        Some(&case.rule_id),
    ) {
        Ok(normalized) => normalized,
        Err(source) => {
            let reason = source.to_string();
            if official_normalization_error_excludes_oracle(&reason) {
                return ScoredCase {
                    bucket: ScoreBucket::NoOfficialOracle,
                    reason: Some(format!(
                        "official results.csv is malformed: {reason}; excluded from supported accuracy"
                    )),
                    ..base
                };
            }
            return ScoredCase {
                reason: Some(format!("official normalization error: {reason}")),
                ..base
            };
        }
    };
    let candidate = match normalize_csv(
        &candidate_report_csv,
        ReportSource::CoreRs,
        Some(&case.rule_id),
    ) {
        Ok(normalized) => normalized,
        Err(source) => {
            return ScoredCase {
                reason: Some(format!("candidate normalization error: {source}")),
                ..base
            }
        }
    };

    if candidate.skipped_row_count > 0 && candidate.issue_count > 0 {
        let skipped_reasons = candidate
            .skipped_reasons
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        return ScoredCase {
            bucket: ScoreBucket::MixedSkippedAndIssues,
            reason: Some("candidate output mixes skipped rows and issue rows".to_owned()),
            skipped_reasons,
            official_issue_count: Some(official.issue_count),
            candidate_issue_count: Some(candidate.issue_count),
            ..base
        };
    } else if candidate.skipped_row_count > 0 {
        let skipped_reasons = candidate
            .skipped_reasons
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        if !options.strict_scoring {
            if let Some(reason) = deferred_default_engine_oracle_gap_skipped_reason(case) {
                return ScoredCase {
                    bucket: ScoreBucket::DeferredOracleGapSkipped,
                    reason: Some(reason),
                    skipped_reasons,
                    official_issue_count: Some(official.issue_count),
                    candidate_issue_count: Some(candidate.issue_count),
                    ..base
                };
            }
        }
        return ScoredCase {
            bucket: ScoreBucket::SkippedUnsupported,
            reason: Some(format!(
                "candidate skipped rows: {}",
                skipped_reasons.join(", ")
            )),
            skipped_reasons,
            official_issue_count: Some(official.issue_count),
            candidate_issue_count: Some(candidate.issue_count),
            ..base
        };
    }

    let mut official_issues = official.issues;
    let (candidate_issues, scoring_normalizations) = if options.strict_scoring {
        (candidate.issues, Vec::new())
    } else {
        let duplicate_sequence_values = duplicate_sequence_values_by_dataset(case);
        let mut candidate_issues = align_candidate_identity_to_official(
            &official_issues,
            candidate.issues,
            &duplicate_sequence_values,
        );
        let scoring_normalizations = normalize_deferred_oracle_gap_issue_identity(
            case,
            &mut official_issues,
            &mut candidate_issues,
        );
        (candidate_issues, scoring_normalizations)
    };
    let execution_provenance_detail = provenance::execution_provenance_detail_for_case(
        &case.rule_id,
        &base.execution_provenance,
        &scoring_normalizations,
    );
    let scoring_policy = provenance::scoring_policy_for_normalizations(&scoring_normalizations);
    let official_issue_count = official_issues.len();
    let candidate_issue_count = candidate_issues.len();
    let (missing, extra) = issue_multiset_diff(official_issues, candidate_issues);
    let missing_count = missing.len();
    let extra_count = extra.len();
    let issue_fingerprint_hash = issue_fingerprint_hash(&missing, &extra);
    if !options.strict_scoring && (!missing.is_empty() || !extra.is_empty()) {
        if official_oracle_fixture_gap_category(case) {
            return ScoredCase {
                bucket: ScoreBucket::DeferredOracleGapSkipped,
                execution_provenance_detail: execution_provenance_detail.clone(),
                scoring_policy: scoring_policy.clone(),
                reason: Some(
                    "official oracle fixture gap; excluded from supported accuracy until upstream oracle/data are reconciled"
                        .to_owned(),
                ),
                skipped_reasons: Vec::new(),
                scoring_normalizations: scoring_normalizations.clone(),
                official_issue_count: Some(official_issue_count),
                candidate_issue_count: Some(candidate_issue_count),
                missing_count: Some(missing_count),
                extra_count: Some(extra_count),
                issue_fingerprint_hash: Some(issue_fingerprint_hash),
                missing,
                extra,
                ..base
            };
        }
        if let Some(reason) = deferred_default_engine_oracle_gap_reason(case) {
            return ScoredCase {
                bucket: ScoreBucket::DeferredOracleGapMismatch,
                execution_provenance_detail: execution_provenance_detail.clone(),
                scoring_policy: scoring_policy.clone(),
                reason: Some(reason.clone()),
                skipped_reasons: Vec::new(),
                scoring_normalizations: scoring_normalizations.clone(),
                official_issue_count: Some(official_issue_count),
                candidate_issue_count: Some(candidate_issue_count),
                missing_count: Some(missing_count),
                extra_count: Some(extra_count),
                issue_fingerprint_hash: Some(issue_fingerprint_hash),
                missing,
                extra,
                ..base
            };
        }
    }
    let bucket = if missing.is_empty() && extra.is_empty() {
        ScoreBucket::SupportedMatch
    } else {
        ScoreBucket::SupportedMismatch
    };

    ScoredCase {
        bucket,
        execution_provenance_detail,
        scoring_policy,
        scoring_normalizations,
        official_issue_count: Some(official_issue_count),
        candidate_issue_count: Some(candidate_issue_count),
        missing_count: Some(missing_count),
        extra_count: Some(extra_count),
        issue_fingerprint_hash: Some(issue_fingerprint_hash),
        missing,
        extra,
        ..base
    }
}

fn issue_multiset_diff(
    official: Vec<IssueKey>,
    candidate: Vec<IssueKey>,
) -> (Vec<IssueKey>, Vec<IssueKey>) {
    let mut official_counts = issue_counts(official);
    let mut candidate_counts = issue_counts(candidate);
    for (issue, official_count) in official_counts.clone() {
        let shared = official_count.min(*candidate_counts.get(&issue).unwrap_or(&0));
        if shared == 0 {
            continue;
        }
        if let Some(count) = official_counts.get_mut(&issue) {
            *count -= shared;
        }
        if let Some(count) = candidate_counts.get_mut(&issue) {
            *count -= shared;
        }
    }
    (
        expand_issue_counts(official_counts),
        expand_issue_counts(candidate_counts),
    )
}

fn issue_counts(issues: Vec<IssueKey>) -> BTreeMap<IssueKey, usize> {
    let mut counts = BTreeMap::new();
    for issue in issues {
        *counts.entry(issue).or_default() += 1;
    }
    counts
}

fn expand_issue_counts(counts: BTreeMap<IssueKey, usize>) -> Vec<IssueKey> {
    counts
        .into_iter()
        .flat_map(|(issue, count)| std::iter::repeat_n(issue, count))
        .collect()
}

fn missing_official_reason(case: &OpenRulesCase, candidate_report_csv: &Path) -> Option<String> {
    if !candidate_report_csv.is_file() {
        return Some("missing official results.csv; candidate report absent".to_owned());
    }
    let candidate = normalize_csv(
        candidate_report_csv,
        ReportSource::CoreRs,
        Some(&case.rule_id),
    )
    .ok()?;
    let candidate_state = if candidate.skipped_row_count > 0 {
        "candidate skipped"
    } else if candidate.issue_count == 0 {
        "candidate empty"
    } else {
        "candidate has issues"
    };
    Some(format!(
        "missing official results.csv; {candidate_state}; excluded from supported accuracy"
    ))
}

fn official_normalization_error_excludes_oracle(reason: &str) -> bool {
    reason.contains("merge conflict markers")
}

pub(crate) fn parse_coverage_ratio(value: &str) -> Result<f64, String> {
    let ratio = value
        .parse::<f64>()
        .map_err(|source| format!("invalid coverage ratio: {source}"))?;
    if ratio.is_finite() && (0.0..=1.0).contains(&ratio) {
        Ok(ratio)
    } else {
        Err("coverage ratio must be finite and between 0.0 and 1.0".to_owned())
    }
}

pub fn issue_fingerprint_hash(missing: &[IssueKey], extra: &[IssueKey]) -> String {
    let mut entries = Vec::with_capacity(missing.len() + extra.len());
    for issue in missing {
        entries.push(issue_fingerprint_entry("missing", issue));
    }
    for issue in extra {
        entries.push(issue_fingerprint_entry("extra", issue));
    }
    entries.sort();

    let mut hash = 0xcbf29ce484222325u64;
    for entry in entries {
        fnv1a_update(&mut hash, entry.as_bytes());
        fnv1a_update(&mut hash, b"\n");
    }
    format!("{hash:016x}")
}

fn issue_fingerprint_entry(kind: &str, issue: &IssueKey) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        kind,
        issue.rule_id,
        issue.dataset,
        issue.domain,
        issue.row,
        issue.variables.join("|"),
        issue.usubjid,
        issue.seq
    )
}

fn fnv1a_update(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

impl Scoreboard {
    #[cfg(test)]
    pub fn new(upstream: UpstreamInfo, cases: Vec<ScoredCase>) -> Self {
        Self::new_with_gate(upstream, cases, None, None, false)
    }

    pub fn new_with_gate(
        upstream: UpstreamInfo,
        cases: Vec<ScoredCase>,
        min_coverage: Option<f64>,
        max_skipped_unsupported: Option<usize>,
        fail_on_deferred_oracle_gap: bool,
    ) -> Self {
        let summary = ScoreSummary::from_cases(&cases);
        let gate = ScoreGate::new(
            &summary,
            min_coverage,
            max_skipped_unsupported,
            fail_on_deferred_oracle_gap,
        );
        let by_scope = summary::grouped_summary(&cases, |case| case.scope.clone());
        let by_case_kind = summary::grouped_summary(&cases, |case| case.case_kind.clone());
        Self {
            upstream,
            summary,
            gate,
            by_scope,
            by_case_kind,
            cases,
        }
    }
}

#[cfg(test)]
mod tests;
