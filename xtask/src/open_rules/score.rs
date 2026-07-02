use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::normalize::{normalize_csv, normalize_scalar, IssueKey, ReportSource};
use crate::open_rules::report::write_scoreboard;
use crate::open_rules::upstream::{load_upstream_info, UpstreamInfo};

mod provenance;
mod summary;
pub use provenance::{
    execution_provenance_for_rule_id, ExecutionProvenance, ExecutionProvenanceDetail,
};
pub use summary::{GroupSummary, ScoreGate, ScoreSummary};

type IssueSignature = (String, String, String, Vec<String>);
type IssueRowSignature = (String, String, String, Vec<String>, String);

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
    #[serde(default)]
    pub missing: Vec<IssueKey>,
    #[serde(default)]
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
    let scored = score_cases(&cases, &args.core_rs_results_root);
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

pub fn score_cases(cases: &[OpenRulesCase], core_rs_results_root: &Path) -> Vec<ScoredCase> {
    cases
        .iter()
        .map(|case| score_case(case, core_rs_results_root))
        .collect()
}

pub fn relative_candidate_report_path(case: &OpenRulesCase) -> PathBuf {
    Path::new(&case.scope)
        .join(&case.rule_id)
        .join(case.case_kind.as_str())
        .join(&case.case_id)
        .join("report.csv")
}

fn score_case(case: &OpenRulesCase, core_rs_results_root: &Path) -> ScoredCase {
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
    let execution_provenance_detail = provenance::execution_provenance_detail_for_case(
        &case.rule_id,
        &base.execution_provenance,
        &scoring_normalizations,
    );
    let official_issue_count = official_issues.len();
    let candidate_issue_count = candidate_issues.len();
    let (missing, extra) = issue_multiset_diff(official_issues, candidate_issues);
    let missing_count = missing.len();
    let extra_count = extra.len();
    let issue_fingerprint_hash = issue_fingerprint_hash(&missing, &extra);
    if !missing.is_empty() || !extra.is_empty() {
        if official_oracle_fixture_gap_category(case) {
            return ScoredCase {
                bucket: ScoreBucket::DeferredOracleGapSkipped,
                execution_provenance_detail: execution_provenance_detail.clone(),
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

fn normalize_deferred_oracle_gap_issue_identity(
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

fn official_oracle_fixture_gap_category(case: &OpenRulesCase) -> bool {
    core_api::rule_id_has_oracle_gap_category(&case.rule_id, "official_oracle_fixture_gap")
}

fn row_locator_oracle_gap_category(case: &OpenRulesCase) -> bool {
    [
        "record_row_locator",
        "defer_unique_set",
        "scope_wide_reference_distinct",
        "defer_duplicate_match_dataset",
    ]
    .into_iter()
    .any(|category| core_api::rule_id_has_oracle_gap_category(&case.rule_id, category))
}

fn clear_issue_record_locators(issues: &mut [IssueKey]) {
    for issue in issues {
        issue.row.clear();
        issue.usubjid.clear();
        issue.seq.clear();
    }
}

fn output_context_variable_oracle_gap_category(case: &OpenRulesCase) -> bool {
    [
        "defer_empty_non_empty",
        "empty_non_empty",
        "defer_positive_zero_probe",
    ]
    .into_iter()
    .any(|category| core_api::rule_id_has_oracle_gap_category(&case.rule_id, category))
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

fn align_candidate_identity_to_official(
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

fn duplicate_sequence_values_by_dataset(case: &OpenRulesCase) -> BTreeSet<(String, String)> {
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

fn deferred_default_engine_oracle_gap_reason(case: &OpenRulesCase) -> Option<String> {
    let reason = deferred_default_engine_oracle_gap_reason_text(case)?;
    Some(format!(
        "{reason}; excluded from supported accuracy until native semantics are verified"
    ))
}

fn deferred_default_engine_oracle_gap_skipped_reason(case: &OpenRulesCase) -> Option<String> {
    let reason = deferred_default_engine_oracle_gap_reason_text(case)?;
    Some(format!(
        "{reason}; candidate skipped; excluded from supported accuracy until native semantics are verified"
    ))
}

fn deferred_default_engine_oracle_gap_reason_text(case: &OpenRulesCase) -> Option<&'static str> {
    [
        ("official_oracle_fixture_gap", "official oracle fixture gap"),
        (
            "defer_empty_non_empty",
            "deferred empty/non_empty oracle semantics",
        ),
        (
            "defer_domain_placeholder_column_ref",
            "deferred domain placeholder column-ref oracle semantics",
        ),
        (
            "defer_not_unique_relationship",
            "deferred not-unique relationship oracle semantics",
        ),
        (
            "defer_duplicate_match_dataset",
            "deferred duplicate match dataset oracle semantics",
        ),
        ("defer_date_operator", "deferred date oracle semantics"),
        ("defer_unique_set", "deferred unique-set oracle semantics"),
        (
            "defer_dy_operation",
            "deferred DY operation oracle semantics",
        ),
        ("defer_sort_operator", "deferred sort oracle semantics"),
        ("defer_etcd_length", "deferred ETCD length oracle semantics"),
        (
            "defer_multi_base_match_dataset",
            "deferred multi-base match dataset oracle semantics",
        ),
        (
            "defer_distinct_operation",
            "deferred distinct operation oracle semantics",
        ),
        (
            "standard_filter_oracle_gap",
            "standard applicability oracle semantics",
        ),
        (
            "required_value_metadata",
            "required value metadata oracle semantics",
        ),
        ("dataset_presence", "dataset presence oracle semantics"),
        ("date_operator", "date oracle semantics"),
        ("domain_presence", "domain presence oracle semantics"),
        ("variable_metadata", "variable metadata oracle semantics"),
        (
            "domain_placeholder_column_ref_comparator",
            "domain placeholder column-ref comparator oracle semantics",
        ),
        ("empty_non_empty", "empty/non_empty oracle semantics"),
        ("missing_column", "missing-column oracle semantics"),
        ("operation", "operation oracle semantics"),
        (
            "scope_wide_reference_distinct",
            "scope-wide reference distinct oracle semantics",
        ),
        (
            "reference_distinct_official_empty",
            "reference distinct official-empty oracle semantics",
        ),
        (
            "reference_distinct_fixture_row",
            "reference distinct fixture row oracle semantics",
        ),
        (
            "reference_distinct_cardinality",
            "reference distinct cardinality oracle semantics",
        ),
        (
            "supported_reference_distinct",
            "reference distinct oracle semantics",
        ),
        ("record_row_locator", "record row locator oracle semantics"),
        (
            "usdm_jsonata_entity_scope",
            "USDM JSONata entity-scope oracle semantics",
        ),
        (
            "record_count_operation",
            "record-count operation oracle semantics",
        ),
        (
            "usdm_join_operation",
            "USDM join operation oracle semantics",
        ),
        ("xhtml_operation", "XHTML operation oracle semantics"),
        (
            "defer_positive_zero_probe",
            "deferred positive-zero probe oracle semantics",
        ),
    ]
    .into_iter()
    .find_map(|(category, reason)| {
        core_api::rule_id_has_oracle_gap_category(&case.rule_id, category).then_some(reason)
    })
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
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::open_rules::discovery::{CaseKind, OpenRulesCase};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use crate::open_rules::discovery::discover_cases;

    use super::*;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn test_upstream() -> UpstreamInfo {
        UpstreamInfo {
            repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
            expected_sha: Some("expected".to_owned()),
            observed_sha: Some("expected".to_owned()),
            lock_path: "tests/open_rules/upstream.lock".into(),
            warnings: Vec::new(),
        }
    }

    fn scored_case(bucket: ScoreBucket, reason: Option<&str>) -> ScoredCase {
        ScoredCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-OPEN-0001".to_owned(),
            case_kind: "negative".to_owned(),
            case_id: "01".to_owned(),
            case_dir: PathBuf::from("case"),
            official_results_csv: PathBuf::from("results.csv"),
            candidate_report_csv: PathBuf::from("report.csv"),
            execution_provenance: ExecutionProvenance::NativeEngine,
            execution_provenance_detail: ExecutionProvenanceDetail::GenericEngine,
            bucket,
            reason: reason.map(str::to_owned),
            skipped_reasons: Vec::new(),
            scoring_normalizations: Vec::new(),
            official_issue_count: None,
            candidate_issue_count: None,
            missing_count: None,
            extra_count: None,
            issue_fingerprint_hash: None,
            missing: Vec::new(),
            extra: Vec::new(),
        }
    }

    fn write_score_fixture(
        root: &Path,
        rule_id: &str,
        case_kind: &str,
        case_id: &str,
        official_csv: &str,
        candidate_csv: &str,
    ) -> OpenRulesCase {
        let case_dir = root
            .join("open/Published")
            .join(rule_id)
            .join(case_kind)
            .join(case_id);
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(case_dir.join("results/results.csv"), official_csv)
            .expect("write official results");
        let candidate_dir = root
            .join("candidate/Published")
            .join(rule_id)
            .join(case_kind)
            .join(case_id);
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(candidate_dir.join("report.csv"), candidate_csv).expect("write candidate report");
        let case_kind = match case_kind {
            "negative" => CaseKind::Negative,
            "positive" => CaseKind::Positive,
            other => panic!("unsupported case kind {other}"),
        };
        OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: rule_id.to_owned(),
            rule_dir: root.join("open/Published").join(rule_id),
            rule_path: root.join("open/Published").join(rule_id).join("rule.yml"),
            case_kind,
            case_id: case_id.to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: true,
        }
    }

    #[test]
    fn scores_match_mismatch_skip_and_harness_errors() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let candidate_root = repo_root().join("tests/fixtures/open_rules_candidate_reports");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");

        let scored = score_cases(&cases, &candidate_root);
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(summary.total_cases, 6);
        assert_eq!(summary.supported_match, 2);
        assert_eq!(summary.official_oracle_match, 2);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
        assert_eq!(summary.supported_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 1);
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.harness_error, 1);
        assert_eq!(summary.supported_accuracy, Some(2.0 / 3.0));
        assert_eq!(summary.coverage, Some(3.0 / 6.0));
        assert!(summary.should_fail());
        let skipped = scored
            .iter()
            .find(|case| case.bucket == ScoreBucket::SkippedUnsupported)
            .expect("skipped case");
        assert_eq!(
            skipped.skipped_reasons,
            vec!["unsupported_operator".to_owned()]
        );
        assert_eq!(
            skipped.reason,
            Some("candidate skipped rows: unsupported_operator".to_owned())
        );
    }

    #[test]
    fn summary_splits_deferred_oracle_gap_skipped_by_review_category() {
        let mut candidate_skipped = scored_case(
            ScoreBucket::DeferredOracleGapSkipped,
            Some("empty/non_empty oracle semantics; candidate skipped"),
        );
        candidate_skipped
            .skipped_reasons
            .push("unsupported operator".to_owned());
        let mut identity_gap = scored_case(
            ScoreBucket::DeferredOracleGapSkipped,
            Some("record row locator oracle semantics"),
        );
        identity_gap.missing_count = Some(1);
        let cases = vec![
            scored_case(
                ScoreBucket::DeferredOracleGapSkipped,
                Some("official oracle fixture gap; excluded from supported accuracy"),
            ),
            scored_case(
                ScoreBucket::DeferredOracleGapSkipped,
                Some("standard applicability oracle semantics; candidate skipped"),
            ),
            candidate_skipped,
            identity_gap,
            scored_case(
                ScoreBucket::DeferredOracleGapSkipped,
                Some("operation oracle semantics"),
            ),
        ];

        let summary = ScoreSummary::from_cases(&cases);

        assert_eq!(summary.deferred_oracle_gap_skipped, 5);
        assert_eq!(
            summary.deferred_oracle_gap_breakdown.official_fixture_gap,
            1
        );
        assert_eq!(
            summary
                .deferred_oracle_gap_breakdown
                .standard_filter_oracle_gap,
            1
        );
        assert_eq!(summary.deferred_oracle_gap_breakdown.candidate_skipped, 1);
        assert_eq!(summary.deferred_oracle_gap_breakdown.oracle_identity_gap, 1);
        assert_eq!(
            summary
                .deferred_oracle_gap_breakdown
                .unverified_semantics_gap,
            1
        );
    }

    #[test]
    fn scores_mixed_skipped_and_issue_candidate_as_failing_bucket() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-MIXED/negative/01");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "rule_id,dataset,row,variables\nCORE-MIXED,DM,1,USUBJID\n",
        )
        .expect("write official results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-MIXED/negative/01");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-MIXED,skipped,DM,DM,,,,0,unsupported_rule_type,,\n\
CORE-MIXED,failed,DM,DM,1,USUBJID,bad,1,,,\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-MIXED".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-MIXED"),
            rule_path: dir.path().join("open/Published/CORE-MIXED/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-MIXED/negative/01/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::MixedSkippedAndIssues);
        assert_eq!(summary.mixed_skipped_and_issues, 1);
        assert!(summary.should_fail());
    }

    #[test]
    fn standard_filter_oracle_gap_skip_is_not_counted_as_skipped_unsupported() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000217",
            "negative",
            "05",
            "rule_id,dataset,row,variables\nCORE-000217,DM,1,AGE\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000217,skipped,DM,DM,,,,0,oracle_semantics_gap,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapSkipped);
        assert_eq!(summary.deferred_oracle_gap_skipped, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert!(!summary.should_fail());
        assert_eq!(
            scored[0].reason,
            Some(
                "standard applicability oracle semantics; candidate skipped; excluded from supported accuracy until native semantics are verified"
                    .to_owned()
            )
        );
    }

    #[test]
    fn official_fixture_gap_skip_is_not_counted_as_skipped_unsupported() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000356",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000356,DM,1,AGE\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000356,skipped,DM,DM,,,,0,unsupported_rule_type,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapSkipped);
        assert_eq!(summary.deferred_oracle_gap_skipped, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert_eq!(
            scored[0].reason,
            Some(
                "official oracle fixture gap; candidate skipped; excluded from supported accuracy until native semantics are verified"
                    .to_owned()
            )
        );
    }

    #[test]
    fn deferred_empty_non_empty_mismatch_is_scored_as_deferred_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000007",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000007,CM,1,CMSTAT\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000007,failed,CM,CM,2,CMSTAT,bad,1,,002,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapMismatch);
        assert_eq!(summary.supported_mismatch, 0);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert!(scored[0].skipped_reasons.is_empty());
        assert_eq!(
            scored[0].reason,
            Some(
                "deferred empty/non_empty oracle semantics; excluded from supported accuracy until native semantics are verified"
                    .to_owned()
            )
        );
    }

    #[test]
    fn deferred_empty_non_empty_match_remains_supported_match() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000648",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000648,DM,1,AGE\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000648,failed,DM,DM,1,AGE,bad,1,,001,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.skipped_unsupported, 0);
    }

    #[test]
    fn empty_non_empty_oracle_gap_ignores_candidate_output_context_variables() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000027",
            "negative",
            "03",
            "rule_id,dataset,row,variables\n\
CORE-000027,TE,1,TEDUR\n\
CORE-000027,TE,1,TEENRL\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000027,failed,TE,TE,1,ETCD|TEDUR|TEENRL,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].scoring_normalizations,
            vec!["output_context_variable_aligned".to_owned()]
        );
        assert_eq!(
            scored[0].execution_provenance_detail,
            ExecutionProvenanceDetail::OracleGapNormalized
        );
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn positive_zero_probe_oracle_gap_ignores_candidate_output_context_variables() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000325",
            "negative",
            "01",
            "rule_id,dataset,row,variables\n\
CORE-000325,DM,1,ARMCD\n\
CORE-000325,TA,3,ARMCD\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000325,failed,DM,DM,1,ARMCD|TXPARMCD|TXVAL,bad,1,,,\n\
CORE-000325,failed,TA,TA,3,ARMCD|TXPARMCD|TXVAL,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].scoring_normalizations,
            vec!["output_context_variable_aligned".to_owned()]
        );
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn direct_oracle_gap_category_mismatch_is_scored_as_deferred_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000237",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000237,PD,1,PDVALMIN\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000237,failed,PD,PD,2,PDVALMIN,bad,1,,002,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapMismatch);
        assert_eq!(summary.supported_mismatch, 0);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert!(scored[0].skipped_reasons.is_empty());
        assert!(scored[0].reason.as_deref().is_some_and(|reason| reason
            .contains("excluded from supported accuracy until native semantics are verified")));
    }

    #[test]
    fn direct_oracle_gap_category_match_remains_supported_match() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000542",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000542,PD,1,PDVALMIN\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000542,failed,PD,PD,1,PDVALMIN,bad,1,,001,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.skipped_unsupported, 0);
    }

    #[test]
    fn official_fixture_gap_is_scored_as_deferred_oracle_gap_skipped() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000049",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000049,LB,,LBIMPLBL\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000049,failed,LB,LB,1,LBUSCHFL,bad,1,,ABC12301001,1\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapSkipped);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
        assert_eq!(summary.deferred_oracle_gap_skipped, 1);
        assert_eq!(
            scored[0].reason,
            Some(
                "official oracle fixture gap; excluded from supported accuracy until upstream oracle/data are reconciled"
                    .to_owned()
            )
        );
    }

    #[test]
    fn supported_reference_distinct_mismatch_is_scored_as_deferred_oracle_gap() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000168",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000168,SV,1,VISIT\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000168,failed,SV,SV,2,VISIT,bad,1,,002,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapMismatch);
        assert_eq!(summary.supported_mismatch, 0);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert!(scored[0].skipped_reasons.is_empty());
    }

    #[test]
    fn reference_distinct_official_empty_gap_uses_specific_reason() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000108",
            "negative",
            "02",
            "rule_id,dataset,row,variables\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000108,failed,DM,DM,2,DTHFL|USUBJID,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapSkipped);
        assert_eq!(
            scored[0].reason,
            Some(
                "official oracle fixture gap; excluded from supported accuracy until upstream oracle/data are reconciled"
                    .to_owned()
            )
        );
    }

    #[test]
    fn official_fixture_gap_takes_precedence_over_reference_distinct_fixture_row_gap() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000770",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000770,TX,8,TXPARMCD\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapSkipped);
        assert!(scored[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("official oracle fixture gap")));
    }

    #[test]
    fn reference_distinct_cardinality_gap_uses_specific_reason() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000168",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000168,LB,395,VISITNUM\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapMismatch);
        assert_eq!(
            scored[0].reason,
            Some(
                "reference distinct cardinality oracle semantics; excluded from supported accuracy until native semantics are verified"
                    .to_owned()
            )
        );
    }

    #[test]
    fn record_row_locator_oracle_gap_matches_when_only_rows_differ() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000137",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000137,EC,12,ECDOSE\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000137,failed,EC,EC,13,ECDOSE,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].scoring_normalizations,
            vec!["row_locator_identity_relaxed".to_owned()]
        );
        assert_eq!(
            scored[0].execution_provenance_detail,
            ExecutionProvenanceDetail::OracleGapNormalized
        );
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn row_locator_oracle_gap_normalization_does_not_hide_issue_count_differences() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000137",
            "negative",
            "01",
            "rule_id,dataset,row,variables\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000137,failed,AE,AE,2,STUDYID,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::DeferredOracleGapMismatch);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 1);
    }

    #[test]
    fn unique_set_oracle_gap_matches_when_only_rows_differ() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000387",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000387,CO,1,USUBJID\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000387,failed,CO,CO,2,USUBJID,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn core_000249_reference_distinct_gap_matches_when_only_rows_differ() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000249",
            "negative",
            "02",
            "rule_id,dataset,row,variables\nCORE-000249,DS,501,VISITDY\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000249,failed,DS,DS,4,VISITDY,bad,1,,,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn core_000269_reference_distinct_gap_matches_when_only_rows_differ() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000269",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000269,LB,584,VISIT\nCORE-000269,LB,584,VISITNUM\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000269,failed,LB,LB,585,VISIT|VISITNUM,bad,1,,CDISC005,293\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(summary.supported_match, 1);
        assert_eq!(summary.deferred_oracle_gap_mismatch, 0);
    }

    #[test]
    fn fail_on_deferred_oracle_gap_makes_score_gate_fail() {
        let dir = tempdir().expect("tempdir");
        let case = write_score_fixture(
            dir.path(),
            "CORE-000168",
            "negative",
            "01",
            "rule_id,dataset,row,variables\nCORE-000168,SV,1,VISIT\n",
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000168,failed,SV,SV,2,VISIT,bad,1,,002,\n",
        );

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let permissive =
            Scoreboard::new_with_gate(test_upstream(), scored.clone(), None, None, false);
        let strict = Scoreboard::new_with_gate(test_upstream(), scored, None, None, true);

        assert_eq!(permissive.summary.deferred_oracle_gap_mismatch, 1);
        assert!(!permissive.gate.deferred_oracle_gap_failed);
        assert!(permissive.gate.should_fail);
        assert!(strict.gate.deferred_oracle_gap_failed);
        assert!(strict.gate.should_fail);
    }

    #[test]
    fn scores_supported_match_with_candidate_execution_provenance() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-PROV/negative/01");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "rule_id,dataset,row,variables\nCORE-PROV,DM,1,USUBJID\n",
        )
        .expect("write official results");
        let candidate_dir = dir.path().join("candidate/Published/CORE-PROV/negative/01");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq,execution_provenance\n\
CORE-PROV,failed,DM,DM,1,USUBJID,bad,1,,,,native_engine\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-PROV".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-PROV"),
            rule_path: dir.path().join("open/Published/CORE-PROV/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-PROV/negative/01/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].execution_provenance,
            ExecutionProvenance::NativeEngine
        );
        assert_eq!(
            scored[0].execution_provenance_detail,
            ExecutionProvenanceDetail::GenericEngine
        );
        assert_eq!(summary.native_engine_supported_match, 1);
        assert_eq!(summary.native_engine_coverage, Some(1.0));
        assert_eq!(
            summary.by_execution_provenance_detail[0].detail,
            ExecutionProvenanceDetail::GenericEngine
        );
    }

    #[test]
    fn candidate_report_without_provenance_column_falls_back_to_rule_id() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-000583/negative/01");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "rule_id,dataset,row,variables\nCORE-000583,TS,1,TSVAL\n",
        )
        .expect("write official results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000583/negative/01");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000583,failed,TS,TS,1,TSVAL,bad,1,,,\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000583".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000583"),
            rule_path: dir.path().join("open/Published/CORE-000583/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-000583/negative/01/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].execution_provenance,
            ExecutionProvenance::RuleIdHandPort
        );
        assert_eq!(
            scored[0].execution_provenance_detail,
            ExecutionProvenanceDetail::RuleIdHandPort
        );
        assert_eq!(summary.rule_id_hand_port_supported_match, 1);
        assert_eq!(summary.unknown_provenance_supported_match, 0);
    }

    #[test]
    fn empty_candidate_report_with_provenance_header_falls_back_to_hand_port_rule_id() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-000583/positive/01");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "rule_id,dataset,row,variables\n",
        )
        .expect("write official results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000583/positive/01");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq,execution_provenance\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000583".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000583"),
            rule_path: dir.path().join("open/Published/CORE-000583/rule.yml"),
            case_kind: CaseKind::Positive,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-000583/positive/01/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].execution_provenance,
            ExecutionProvenance::RuleIdHandPort
        );
        assert_eq!(summary.rule_id_hand_port_supported_match, 1);
        assert_eq!(summary.unknown_provenance_supported_match, 0);
    }

    #[test]
    fn empty_candidate_report_with_provenance_header_falls_back_to_native_rule_id() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-PROV/positive/01");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "rule_id,dataset,row,variables\n",
        )
        .expect("write official results");
        let candidate_dir = dir.path().join("candidate/Published/CORE-PROV/positive/01");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq,execution_provenance\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-PROV".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-PROV"),
            rule_path: dir.path().join("open/Published/CORE-PROV/rule.yml"),
            case_kind: CaseKind::Positive,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-PROV/positive/01/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(
            scored[0].execution_provenance,
            ExecutionProvenance::NativeEngine
        );
        assert_eq!(summary.native_engine_supported_match, 1);
        assert_eq!(summary.unknown_provenance_supported_match, 0);
    }

    #[test]
    fn official_merge_conflict_marker_is_no_official_oracle_not_harness_error() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("open/Published/CORE-000159/negative/02");
        fs::create_dir_all(case_dir.join("results")).expect("create official results dir");
        fs::write(
            case_dir.join("results/results.csv"),
            "Dataset,Record,Variable,Value\n<<<<<<< HEAD\nLB,0,LBTESTCD,OTHER\n=======\nLB.csv,1,LBTESTCD,OTHER\n>>>>>>> main\n",
        )
        .expect("write official results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000159/negative/02");
        fs::create_dir_all(&candidate_dir).expect("create candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000159,failed,LB,LB,1,LBTESTCD,bad,1,,,\n",
        )
        .expect("write candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000159".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000159"),
            rule_path: dir.path().join("open/Published/CORE-000159/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "02".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: dir
                .path()
                .join("open/Published/CORE-000159/negative/02/results/results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.harness_error, 0);
        assert!(scored[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("official results.csv is malformed")));
    }

    #[test]
    fn missing_official_empty_candidate_is_no_official_oracle() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000016/negative/03");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000016/negative/03");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000016".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000016"),
            rule_path: dir.path().join("open/Published/CORE-000016/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "03".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(
            scored[0].reason,
            Some(
                "missing official results.csv; candidate empty; excluded from supported accuracy"
                    .to_owned()
            )
        );
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.official_oracle_match, 0);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
        assert_eq!(summary.harness_error, 0);
        assert!(!summary.should_fail());
    }

    #[test]
    fn missing_official_positive_empty_candidate_is_no_official_oracle() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000016/positive/03");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000016/positive/03");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000016,passed,CM,CM,,,ok,0,,,\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000016".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000016"),
            rule_path: dir.path().join("open/Published/CORE-000016/rule.yml"),
            case_kind: CaseKind::Positive,
            case_id: "03".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(scored[0].official_issue_count, None);
        assert_eq!(scored[0].candidate_issue_count, None);
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.official_oracle_match, 0);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
    }

    #[test]
    fn missing_official_negative_candidate_issues_are_no_official_oracle() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000016/negative/03");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000016/negative/03");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000016,failed,CM,CM,1,CMSTDTC,text,1,,SUBJ001,1\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000016".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000016"),
            rule_path: dir.path().join("open/Published/CORE-000016/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "03".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(
            scored[0].reason,
            Some(
                "missing official results.csv; candidate has issues; excluded from supported accuracy"
                    .to_owned()
            )
        );
        assert_eq!(scored[0].official_issue_count, None);
        assert_eq!(scored[0].candidate_issue_count, None);
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.official_oracle_match, 0);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
    }

    #[test]
    fn missing_official_skipped_candidate_is_no_official_oracle() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000107/positive/01");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000107/positive/01");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000107,skipped,DM,DM,,,skipped,0,unsupported_operator,,\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000107".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000107"),
            rule_path: dir.path().join("open/Published/CORE-000107/rule.yml"),
            case_kind: CaseKind::Positive,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(
            scored[0].reason,
            Some(
                "missing official results.csv; candidate skipped; excluded from supported accuracy"
                    .to_owned()
            )
        );
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.skipped_unsupported, 0);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.official_oracle_match, 0);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
    }

    #[test]
    fn missing_official_missing_candidate_is_no_official_oracle() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000638/negative/data");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000638".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000638"),
            rule_path: dir.path().join("open/Published/CORE-000638/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "data".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(scored[0].bucket, ScoreBucket::NoOfficialOracle);
        assert_eq!(
            scored[0].reason,
            Some("missing official results.csv; candidate report absent".to_owned())
        );
        assert_eq!(summary.no_official_oracle, 1);
        assert_eq!(summary.harness_error, 0);
        assert_eq!(summary.supported_match, 0);
        assert_eq!(summary.official_oracle_match, 0);
        assert_eq!(summary.synthetic_oracle_match, 0);
        assert_eq!(summary.unverified_synthetic_oracle_match, 0);
    }

    #[test]
    fn candidate_report_path_mirrors_case_identity() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");
        let case = cases
            .iter()
            .find(|case| case.rule_id == "CORE-000001" && case.case_kind.as_str() == "positive")
            .expect("positive case");

        assert_eq!(
            relative_candidate_report_path(case),
            Path::new("Published")
                .join("CORE-000001")
                .join("positive")
                .join("01")
                .join("report.csv")
        );
    }

    #[test]
    fn scores_match_when_official_lacks_subject_and_sequence_columns() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000001/negative/01");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000001/negative/01");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nIE,1,IECAT,INCLUSION\nIE,1,IEORRES,Y\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,failed,IE,IE,1,IECAT|IEORRES,text,1,,SUBJ001,1\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000001".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000001"),
            rule_path: dir.path().join("open/Published/CORE-000001/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }

    #[test]
    fn scores_match_when_candidate_seq_identifies_duplicate_record() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000249/negative/03");
        let data_dir = case_dir.join("data");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000249/negative/03");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            data_dir.join("lb.csv"),
            "STUDYID,DOMAIN,USUBJID,LBSEQ,VISITNUM,VISITDY\n\
             S1,LB,SUBJ001,1,99999,-15\n\
             S1,LB,SUBJ001,2,200,1\n\
             S1,LB,SUBJ001,3,2200,141\n\
             S1,LB,SUBJ001,4,2900,213\n\
             S1,LB,SUBJ001,2,200,-15\n",
        )
        .expect("data csv");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nLB,2,VISITNUM,200\nLB,2,VISITDY,-15\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000249,failed,LB,LB,5,VISITDY|VISITNUM,text,1,,SUBJ001,2\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000249".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000249"),
            rule_path: dir.path().join("open/Published/CORE-000249/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "03".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: data_dir.clone(),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: vec![data_dir.join("lb.csv")],
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(scored[0].official_issue_count, Some(2));
        assert_eq!(scored[0].candidate_issue_count, Some(2));
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }

    #[test]
    fn scores_keep_physical_row_when_candidate_seq_is_not_duplicated() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000013/negative/01");
        let data_dir = case_dir.join("data");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000013/negative/01");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            data_dir.join("ae.csv"),
            "STUDYID,DOMAIN,USUBJID,AESEQ,AESTAT\n\
             S1,AE,SUBJ001,1,NOT DONE\n\
             S1,AE,SUBJ001,2,NOT DONE\n\
             S1,AE,SUBJ001,11,NOT DONE\n",
        )
        .expect("data csv");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nAE,11,AESTAT,NOT DONE\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000013,failed,AE,AE,3,AESTAT,text,1,,SUBJ001,11\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000013".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000013"),
            rule_path: dir.path().join("open/Published/CORE-000013/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: data_dir.clone(),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: vec![data_dir.join("ae.csv")],
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMismatch);
        assert_eq!(scored[0].missing.len(), 1);
        assert_eq!(scored[0].extra.len(), 1);
    }

    #[test]
    fn scores_keep_physical_row_when_it_already_matches_official_issue() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000085/negative/02");
        let data_dir = case_dir.join("data");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000085/negative/02");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            data_dir.join("ce.csv"),
            "STUDYID,DOMAIN,USUBJID,CESEQ,CESTRTPT,CESTTPT\n\
             S1,CE,SUBJ001,1,,\n\
             S1,CE,SUBJ002,2,,\n\
             S1,CE,SUBJ003,3,,\n\
             S1,CE,SUBJ004,14,,FIRST DOSE\n\
             S1,CE,SUBJ005,5,,\n\
             S1,CE,SUBJ006,6,,\n\
             S1,CE,SUBJ007,7,,\n\
             S1,CE,SUBJ008,4,,\n\
             S1,CE,SUBJ009,9,,\n\
             S1,CE,SUBJ010,10,,\n\
             S1,CE,SUBJ011,4,,FIRST DOSE\n",
        )
        .expect("data csv");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\n\
             CE,4,CESTRTPT,\n\
             CE,4,CESTTPT,FIRST DOSE\n\
             CE,11,CESTRTPT,\n\
             CE,11,CESTTPT,FIRST DOSE\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000085,failed,CE,CE,4,CESTRTPT|CESTTPT,text,2,,SUBJ004,14\n\
             CORE-000085,failed,CE,CE,11,CESTRTPT|CESTTPT,text,2,,SUBJ011,4\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000085".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000085"),
            rule_path: dir.path().join("open/Published/CORE-000085/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "02".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: data_dir.clone(),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: vec![data_dir.join("ce.csv")],
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }

    #[test]
    fn scores_mismatch_when_wildcard_official_has_duplicate_candidate_issues() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000012/negative/01");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000012/negative/01");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nAE,,AEOCCUR,Y\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000012,failed,AE,AE,1,AEOCCUR,text,2,,SUBJ001,1\n\
             CORE-000012,failed,AE,AE,2,AEOCCUR,text,2,,SUBJ001,2\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000012".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000012"),
            rule_path: dir.path().join("open/Published/CORE-000012/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMismatch);
        assert_eq!(scored[0].official_issue_count, Some(1));
        assert_eq!(scored[0].candidate_issue_count, Some(2));
        assert!(scored[0].missing.is_empty());
        assert_eq!(scored[0].extra.len(), 1);
    }

    #[test]
    fn scores_match_when_official_issue_has_no_location_fields() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-001076/negative/01");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-001076/negative/01");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            official_dir.join("results.csv"),
            "path,attribute,value\n,parent_entity,InterventionalStudyDesign\n,id,Activity_1\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-001076,failed,ACTIVITY,ACTIVITY,32,parent_entity|id,text,1,,,\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-001076".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-001076"),
            rule_path: dir.path().join("open/Published/CORE-001076/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(scored[0].official_issue_count, Some(2));
        assert_eq!(scored[0].candidate_issue_count, Some(2));
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }

    #[test]
    fn scores_mismatch_when_candidate_rows_have_constant_offset() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-000025/negative/01");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir
            .path()
            .join("candidate/Published/CORE-000025/negative/01");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nIE,2,IEORRES,Y\nIE,2,IESTRESC,Yup\nIE,3,IEORRES,Yes\nIE,3,IESTRESC,Yippy\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000025,failed,IE,IE,1,IEORRES|IESTRESC,text,2,,SUBJ001,1\n\
             CORE-000025,failed,IE,IE,2,IEORRES|IESTRESC,text,2,,SUBJ002,1\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000025".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-000025"),
            rule_path: dir.path().join("open/Published/CORE-000025/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMismatch);
        assert_eq!(scored[0].official_issue_count, Some(4));
        assert_eq!(scored[0].candidate_issue_count, Some(4));
        assert_eq!(scored[0].missing.len(), 2);
        assert_eq!(scored[0].extra.len(), 2);
    }

    #[test]
    fn scores_multiset_issue_counts_not_unique_issue_sets() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir
            .path()
            .join("open")
            .join("Published/CORE-DUP/negative/01");
        let official_dir = case_dir.join("results");
        let candidate_dir = dir.path().join("candidate/Published/CORE-DUP/negative/01");
        fs::create_dir_all(&official_dir).expect("official dir");
        fs::create_dir_all(&candidate_dir).expect("candidate dir");
        fs::write(
            official_dir.join("results.csv"),
            "Dataset,Record,Variable,Value\nDM,1,USUBJID,bad\nDM,1,USUBJID,bad\n",
        )
        .expect("official results");
        fs::write(
            candidate_dir.join("report.csv"),
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-DUP,failed,DM,DM,1,USUBJID,text,1,,,\n",
        )
        .expect("candidate report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-DUP".to_owned(),
            rule_dir: dir.path().join("open/Published/CORE-DUP"),
            rule_path: dir.path().join("open/Published/CORE-DUP/rule.yml"),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: BTreeMap::new(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: official_dir.join("results.csv"),
            has_official_results: true,
        };

        let scored = score_cases(&[case], &dir.path().join("candidate"));

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMismatch);
        assert_eq!(scored[0].official_issue_count, Some(2));
        assert_eq!(scored[0].candidate_issue_count, Some(1));
        assert_eq!(scored[0].missing.len(), 1);
        assert!(scored[0].extra.is_empty());
    }
}
