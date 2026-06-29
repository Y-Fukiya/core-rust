use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::normalize::{normalize_csv, IssueKey, ReportSource};
use crate::open_rules::report::write_scoreboard;
use crate::open_rules::upstream::{load_upstream_info, UpstreamInfo};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBucket {
    SupportedMatch,
    SupportedMismatch,
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
    pub bucket: ScoreBucket,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_reasons: Vec<String>,
    pub official_issue_count: Option<usize>,
    pub candidate_issue_count: Option<usize>,
    pub missing: Vec<IssueKey>,
    pub extra: Vec<IssueKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreSummary {
    pub total_cases: usize,
    pub supported_match: usize,
    #[serde(default)]
    pub official_oracle_match: usize,
    #[serde(default)]
    pub synthetic_oracle_match: usize,
    #[serde(default)]
    pub unverified_synthetic_oracle_match: usize,
    pub supported_mismatch: usize,
    pub skipped_unsupported: usize,
    #[serde(default)]
    pub mixed_skipped_and_issues: usize,
    #[serde(default)]
    pub no_official_oracle: usize,
    pub harness_error: usize,
    pub supported_accuracy: Option<f64>,
    pub coverage: Option<f64>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ScoreGate {
    pub min_coverage: Option<f64>,
    pub max_skipped_unsupported: Option<usize>,
    pub coverage_threshold_failed: bool,
    pub skipped_unsupported_threshold_failed: bool,
    pub should_fail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupSummary {
    pub name: String,
    pub summary: ScoreSummary,
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
    let base = ScoredCase {
        scope: case.scope.clone(),
        rule_id: case.rule_id.clone(),
        case_kind: case.case_kind.as_str().to_owned(),
        case_id: case.case_id.clone(),
        case_dir: case.case_dir.clone(),
        official_results_csv: case.official_results_csv.clone(),
        candidate_report_csv: candidate_report_csv.clone(),
        bucket: ScoreBucket::HarnessError,
        reason: None,
        skipped_reasons: Vec::new(),
        official_issue_count: None,
        candidate_issue_count: None,
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
            return ScoredCase {
                reason: Some(format!("official normalization error: {source}")),
                ..base
            }
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

    let official_issues = official.issues;
    let candidate_issues = align_candidate_identity_to_official(&official_issues, candidate.issues);
    let official_issue_count = official_issues.len();
    let candidate_issue_count = candidate_issues.len();
    let (missing, extra) = issue_multiset_diff(official_issues, candidate_issues);
    let bucket = if missing.is_empty() && extra.is_empty() {
        ScoreBucket::SupportedMatch
    } else {
        ScoreBucket::SupportedMismatch
    };

    ScoredCase {
        bucket,
        official_issue_count: Some(official_issue_count),
        candidate_issue_count: Some(candidate_issue_count),
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
            } else if !compare_usubjid {
                issue.usubjid.clear();
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

fn issue_signature(issue: &IssueKey) -> (String, String, String, Vec<String>) {
    (
        issue.rule_id.clone(),
        issue.dataset.clone(),
        issue.domain.clone(),
        issue.variables.clone(),
    )
}

impl ScoreSummary {
    pub fn from_cases(cases: &[ScoredCase]) -> Self {
        let mut counts = BTreeMap::<&'static str, usize>::new();
        for case in cases {
            *counts.entry(case.bucket.as_str()).or_default() += 1;
        }
        let supported_match = *counts.get("supported_match").unwrap_or(&0);
        let synthetic_oracle_match = cases
            .iter()
            .filter(|case| {
                case.bucket == ScoreBucket::SupportedMatch && case_is_synthetic_oracle(case)
            })
            .count();
        let unverified_synthetic_oracle_match = cases
            .iter()
            .filter(|case| {
                case.bucket == ScoreBucket::SupportedMatch
                    && case_is_unverified_synthetic_oracle(case)
            })
            .count();
        let official_oracle_match = supported_match.saturating_sub(synthetic_oracle_match);
        let supported_mismatch = *counts.get("supported_mismatch").unwrap_or(&0);
        let skipped_unsupported = *counts.get("skipped_unsupported").unwrap_or(&0);
        let mixed_skipped_and_issues = *counts.get("mixed_skipped_and_issues").unwrap_or(&0);
        let no_official_oracle = *counts.get("no_official_oracle").unwrap_or(&0);
        let harness_error = *counts.get("harness_error").unwrap_or(&0);
        let supported = supported_match + supported_mismatch;
        let total_cases = cases.len();
        Self {
            total_cases,
            supported_match,
            official_oracle_match,
            synthetic_oracle_match,
            unverified_synthetic_oracle_match,
            supported_mismatch,
            skipped_unsupported,
            mixed_skipped_and_issues,
            no_official_oracle,
            harness_error,
            supported_accuracy: (supported > 0).then(|| supported_match as f64 / supported as f64),
            coverage: (total_cases > 0).then(|| supported as f64 / total_cases as f64),
        }
    }

    pub fn should_fail(&self) -> bool {
        self.supported_mismatch > 0
            || self.harness_error > 0
            || self.no_official_oracle > 0
            || self.mixed_skipped_and_issues > 0
    }
}

fn coverage_threshold_failed(summary: &ScoreSummary, min_coverage: Option<f64>) -> bool {
    let Some(min_coverage) = min_coverage else {
        return false;
    };
    summary.coverage.unwrap_or(0.0) < min_coverage
}

fn skipped_unsupported_threshold_failed(
    summary: &ScoreSummary,
    max_skipped_unsupported: Option<usize>,
) -> bool {
    let Some(max_skipped_unsupported) = max_skipped_unsupported else {
        return false;
    };
    summary.skipped_unsupported > max_skipped_unsupported
}

fn case_is_synthetic_oracle(case: &ScoredCase) -> bool {
    case.reason
        .as_deref()
        .is_some_and(|reason| reason.contains("synthetic"))
}

fn case_is_unverified_synthetic_oracle(case: &ScoredCase) -> bool {
    case.reason
        .as_deref()
        .is_some_and(|reason| reason.contains("unverified synthetic"))
}

impl Scoreboard {
    #[cfg(test)]
    pub fn new(upstream: UpstreamInfo, cases: Vec<ScoredCase>) -> Self {
        Self::new_with_gate(upstream, cases, None, None)
    }

    pub fn new_with_gate(
        upstream: UpstreamInfo,
        cases: Vec<ScoredCase>,
        min_coverage: Option<f64>,
        max_skipped_unsupported: Option<usize>,
    ) -> Self {
        let summary = ScoreSummary::from_cases(&cases);
        let gate = ScoreGate::new(&summary, min_coverage, max_skipped_unsupported);
        let by_scope = grouped_summary(&cases, |case| case.scope.clone());
        let by_case_kind = grouped_summary(&cases, |case| case.case_kind.clone());
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

impl ScoreGate {
    fn new(
        summary: &ScoreSummary,
        min_coverage: Option<f64>,
        max_skipped_unsupported: Option<usize>,
    ) -> Self {
        let coverage_threshold_failed = coverage_threshold_failed(summary, min_coverage);
        let skipped_unsupported_threshold_failed =
            skipped_unsupported_threshold_failed(summary, max_skipped_unsupported);
        Self {
            min_coverage,
            max_skipped_unsupported,
            coverage_threshold_failed,
            skipped_unsupported_threshold_failed,
            should_fail: summary.should_fail()
                || coverage_threshold_failed
                || skipped_unsupported_threshold_failed,
        }
    }
}

fn grouped_summary(
    cases: &[ScoredCase],
    mut key: impl FnMut(&ScoredCase) -> String,
) -> Vec<GroupSummary> {
    let mut groups = BTreeMap::<String, Vec<ScoredCase>>::new();
    for case in cases {
        groups.entry(key(case)).or_default().push(case.clone());
    }
    groups
        .into_iter()
        .map(|(name, cases)| GroupSummary {
            name,
            summary: ScoreSummary::from_cases(&cases),
        })
        .collect()
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
        assert!(summary.should_fail());
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
