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

    let candidate_issues = align_candidate_identity_to_official(&official.issues, candidate.issues);
    let candidate_issues = align_candidate_rows_to_official(&official.issues, candidate_issues);
    let official_set = official.issues.into_iter().collect::<BTreeSet<_>>();
    let candidate_set = candidate_issues.into_iter().collect::<BTreeSet<_>>();
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

fn align_candidate_identity_to_official(
    official: &[IssueKey],
    candidate: Vec<IssueKey>,
) -> Vec<IssueKey> {
    let compare_usubjid = official.iter().any(|issue| !issue.usubjid.is_empty());
    let compare_seq = official.iter().any(|issue| !issue.seq.is_empty());
    let official_dataset_issues = official
        .iter()
        .filter(|issue| issue.row.is_empty() && issue.usubjid.is_empty() && issue.seq.is_empty())
        .map(issue_signature)
        .collect::<BTreeSet<_>>();

    candidate
        .into_iter()
        .map(|mut issue| {
            if official_dataset_issues.contains(&issue_signature(&issue)) {
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

fn issue_signature(issue: &IssueKey) -> (String, String, String, Vec<String>) {
    (
        issue.rule_id.clone(),
        issue.dataset.clone(),
        issue.domain.clone(),
        issue.variables.clone(),
    )
}

fn align_candidate_rows_to_official(
    official: &[IssueKey],
    candidate: Vec<IssueKey>,
) -> Vec<IssueKey> {
    if official.len() != candidate.len() || candidate.len() > 256 {
        return candidate;
    }

    let official_set = official.iter().cloned().collect::<BTreeSet<_>>();
    let candidate_set = candidate.iter().cloned().collect::<BTreeSet<_>>();
    if candidate_set == official_set {
        return candidate;
    }

    for offset in [1, -1] {
        let Some(shifted) = shift_candidate_rows(&candidate, offset) else {
            continue;
        };
        let shifted_set = shifted.iter().cloned().collect::<BTreeSet<_>>();
        if shifted_set == official_set {
            return shifted;
        }
    }

    candidate
}

fn shift_candidate_rows(candidate: &[IssueKey], offset: i64) -> Option<Vec<IssueKey>> {
    candidate
        .iter()
        .map(|issue| {
            let mut shifted = issue.clone();
            if !shifted.row.is_empty() {
                let row = shifted.row.parse::<i64>().ok()?;
                let row = row + offset;
                if row <= 0 {
                    return None;
                }
                shifted.row = row.to_string();
            }
            Some(shifted)
        })
        .collect()
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

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
    fn scores_match_when_official_dataset_issue_has_candidate_record_issues() {
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

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(scored[0].official_issue_count, Some(1));
        assert_eq!(scored[0].candidate_issue_count, Some(1));
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }

    #[test]
    fn scores_match_when_candidate_rows_have_constant_offset() {
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

        assert_eq!(scored[0].bucket, ScoreBucket::SupportedMatch);
        assert_eq!(scored[0].official_issue_count, Some(4));
        assert_eq!(scored[0].candidate_issue_count, Some(4));
        assert!(scored[0].missing.is_empty());
        assert!(scored[0].extra.is_empty());
    }
}
