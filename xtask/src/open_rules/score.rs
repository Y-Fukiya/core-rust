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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBucket {
    SupportedMatch,
    SupportedMismatch,
    SkippedUnsupported,
    HarnessError,
}

impl ScoreBucket {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SupportedMatch => "supported_match",
            Self::SupportedMismatch => "supported_mismatch",
            Self::SkippedUnsupported => "skipped_unsupported",
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
    pub official_issue_count: Option<usize>,
    pub candidate_issue_count: Option<usize>,
    pub missing: Vec<IssueKey>,
    pub extra: Vec<IssueKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreSummary {
    pub total_cases: usize,
    pub supported_match: usize,
    pub supported_mismatch: usize,
    pub skipped_unsupported: usize,
    pub harness_error: usize,
    pub supported_accuracy: Option<f64>,
    pub coverage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scoreboard {
    pub upstream: UpstreamInfo,
    pub summary: ScoreSummary,
    pub by_scope: Vec<GroupSummary>,
    pub by_case_kind: Vec<GroupSummary>,
    pub cases: Vec<ScoredCase>,
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
    let scoreboard = Scoreboard::new(upstream, scored);
    write_scoreboard(&args.out, &scoreboard)?;
    Ok(scoreboard.summary.should_fail())
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
        official_issue_count: None,
        candidate_issue_count: None,
        missing: Vec::new(),
        extra: Vec::new(),
    };

    if !case.official_results_csv.is_file() {
        return ScoredCase {
            reason: Some("missing official results.csv".to_owned()),
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

    if candidate.skipped_row_count > 0 {
        return ScoredCase {
            bucket: ScoreBucket::SkippedUnsupported,
            reason: Some("candidate output contains skipped rows".to_owned()),
            official_issue_count: Some(official.issue_count),
            candidate_issue_count: Some(candidate.issue_count),
            ..base
        };
    }

    let official_set = official.issues.into_iter().collect::<BTreeSet<_>>();
    let candidate_set = candidate.issues.into_iter().collect::<BTreeSet<_>>();
    let missing = official_set
        .difference(&candidate_set)
        .cloned()
        .collect::<Vec<_>>();
    let extra = candidate_set
        .difference(&official_set)
        .cloned()
        .collect::<Vec<_>>();
    let bucket = if missing.is_empty() && extra.is_empty() {
        ScoreBucket::SupportedMatch
    } else {
        ScoreBucket::SupportedMismatch
    };

    ScoredCase {
        bucket,
        official_issue_count: Some(official_set.len()),
        candidate_issue_count: Some(candidate_set.len()),
        missing,
        extra,
        ..base
    }
}

impl ScoreSummary {
    pub fn from_cases(cases: &[ScoredCase]) -> Self {
        let mut counts = BTreeMap::<&'static str, usize>::new();
        for case in cases {
            *counts.entry(case.bucket.as_str()).or_default() += 1;
        }
        let supported_match = *counts.get("supported_match").unwrap_or(&0);
        let supported_mismatch = *counts.get("supported_mismatch").unwrap_or(&0);
        let skipped_unsupported = *counts.get("skipped_unsupported").unwrap_or(&0);
        let harness_error = *counts.get("harness_error").unwrap_or(&0);
        let supported = supported_match + supported_mismatch;
        let total_cases = cases.len();
        Self {
            total_cases,
            supported_match,
            supported_mismatch,
            skipped_unsupported,
            harness_error,
            supported_accuracy: (supported > 0).then(|| supported_match as f64 / supported as f64),
            coverage: (total_cases > 0).then(|| supported as f64 / total_cases as f64),
        }
    }

    pub fn should_fail(&self) -> bool {
        self.supported_mismatch > 0 || self.harness_error > 0
    }
}

impl Scoreboard {
    pub fn new(upstream: UpstreamInfo, cases: Vec<ScoredCase>) -> Self {
        let summary = ScoreSummary::from_cases(&cases);
        let by_scope = grouped_summary(&cases, |case| case.scope.clone());
        let by_case_kind = grouped_summary(&cases, |case| case.case_kind.clone());
        Self {
            upstream,
            summary,
            by_scope,
            by_case_kind,
            cases,
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
    use std::path::Path;

    use pretty_assertions::assert_eq;

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
        assert_eq!(summary.supported_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 1);
        assert_eq!(summary.harness_error, 2);
        assert_eq!(summary.supported_accuracy, Some(2.0 / 3.0));
        assert_eq!(summary.coverage, Some(3.0 / 6.0));
        assert!(summary.should_fail());
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
}
