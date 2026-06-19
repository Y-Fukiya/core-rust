//! Open Rules baseline comparison.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::score::{ScoreBucket, Scoreboard, ScoredCase};

#[derive(Debug, Clone, Parser)]
pub struct BaselineArgs {
    #[arg(long, value_name = "FILE")]
    pub scoreboard: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub baseline: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineReport {
    pub regressions: Vec<BaselineDifference>,
    pub improvements: Vec<BaselineDifference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineDifference {
    pub case_key: String,
    pub baseline_bucket: Option<ScoreBucket>,
    pub current_bucket: Option<ScoreBucket>,
    pub message: String,
}

pub fn run(args: BaselineArgs) -> Result<bool> {
    let baseline = read_scoreboard(&args.baseline)?;
    let current = read_scoreboard(&args.scoreboard)?;
    let report = compare_scoreboards(&baseline, &current);
    println!(
        "open-rules baseline: {} regression(s), {} improvement(s)",
        report.regressions.len(),
        report.improvements.len()
    );
    for regression in &report.regressions {
        println!(
            "regression {}: {} -> {} ({})",
            regression.case_key,
            bucket_name(&regression.baseline_bucket),
            bucket_name(&regression.current_bucket),
            regression.message
        );
    }
    for improvement in &report.improvements {
        println!(
            "improvement {}: {} -> {}",
            improvement.case_key,
            bucket_name(&improvement.baseline_bucket),
            bucket_name(&improvement.current_bucket)
        );
    }
    Ok(report.should_fail())
}

pub fn compare_scoreboards(baseline: &Scoreboard, current: &Scoreboard) -> BaselineReport {
    let baseline_cases = baseline
        .cases
        .iter()
        .map(|case| (case_key(case), case))
        .collect::<BTreeMap<_, _>>();
    let current_cases = current
        .cases
        .iter()
        .map(|case| (case_key(case), case))
        .collect::<BTreeMap<_, _>>();

    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let keys = baseline_cases
        .keys()
        .chain(current_cases.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for key in keys {
        let baseline_case = baseline_cases.get(&key).copied();
        let current_case = current_cases.get(&key).copied();
        match (baseline_case, current_case) {
            (Some(baseline_case), Some(current_case)) => {
                if is_improvement(&baseline_case.bucket, &current_case.bucket) {
                    improvements.push(difference(
                        key,
                        Some(&baseline_case.bucket),
                        Some(&current_case.bucket),
                        "case improved to supported_match",
                    ));
                } else if is_regression(&baseline_case.bucket, &current_case.bucket) {
                    regressions.push(difference(
                        key,
                        Some(&baseline_case.bucket),
                        Some(&current_case.bucket),
                        "case bucket regressed",
                    ));
                }
            }
            (Some(baseline_case), None) => regressions.push(difference(
                key,
                Some(&baseline_case.bucket),
                None,
                "baseline case is missing from current scoreboard",
            )),
            (None, Some(current_case)) => {
                if is_failing_new_bucket(&current_case.bucket) {
                    regressions.push(difference(
                        key,
                        None,
                        Some(&current_case.bucket),
                        "new failing case appeared",
                    ));
                }
            }
            (None, None) => {}
        }
    }

    BaselineReport {
        regressions,
        improvements,
    }
}

impl BaselineReport {
    pub fn should_fail(&self) -> bool {
        !self.regressions.is_empty()
    }
}

fn read_scoreboard(path: &PathBuf) -> Result<Scoreboard> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("parse {}", path.display()))
}

fn is_improvement(baseline: &ScoreBucket, current: &ScoreBucket) -> bool {
    *current == ScoreBucket::SupportedMatch && *baseline != ScoreBucket::SupportedMatch
}

fn is_regression(baseline: &ScoreBucket, current: &ScoreBucket) -> bool {
    if baseline == current {
        return false;
    }
    if *baseline == ScoreBucket::SupportedMatch {
        return true;
    }
    is_failing_new_bucket(current)
}

fn is_failing_new_bucket(bucket: &ScoreBucket) -> bool {
    matches!(
        bucket,
        ScoreBucket::SupportedMismatch | ScoreBucket::HarnessError
    )
}

fn difference(
    case_key: String,
    baseline_bucket: Option<&ScoreBucket>,
    current_bucket: Option<&ScoreBucket>,
    message: &str,
) -> BaselineDifference {
    BaselineDifference {
        case_key,
        baseline_bucket: baseline_bucket.cloned(),
        current_bucket: current_bucket.cloned(),
        message: message.to_owned(),
    }
}

fn case_key(case: &ScoredCase) -> String {
    format!(
        "{}/{}/{}/{}",
        case.scope, case.rule_id, case.case_kind, case.case_id
    )
}

fn bucket_name(bucket: &Option<ScoreBucket>) -> &'static str {
    bucket
        .as_ref()
        .map(ScoreBucket::as_str)
        .unwrap_or("missing")
}

#[cfg(test)]
mod tests {
    use crate::open_rules::score::{ScoreBucket, Scoreboard, ScoredCase};
    use crate::open_rules::upstream::UpstreamInfo;

    use super::*;

    fn scoreboard(bucket: ScoreBucket) -> Scoreboard {
        Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("expected".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: Vec::new(),
            },
            vec![ScoredCase {
                scope: "Published".to_owned(),
                rule_id: "CORE-OPEN-0001".to_owned(),
                case_kind: "negative".to_owned(),
                case_id: "01".to_owned(),
                case_dir: "case".into(),
                official_results_csv: "official.csv".into(),
                candidate_report_csv: "report.csv".into(),
                bucket,
                reason: None,
                skipped_reasons: Vec::new(),
                official_issue_count: Some(1),
                candidate_issue_count: Some(1),
                missing: Vec::new(),
                extra: Vec::new(),
            }],
        )
    }

    #[test]
    fn baseline_passes_when_case_buckets_match() {
        let report = compare_scoreboards(
            &scoreboard(ScoreBucket::SupportedMatch),
            &scoreboard(ScoreBucket::SupportedMatch),
        );

        assert!(!report.should_fail());
        assert!(report.regressions.is_empty());
        assert!(report.improvements.is_empty());
    }

    #[test]
    fn baseline_fails_when_supported_match_regresses() {
        let report = compare_scoreboards(
            &scoreboard(ScoreBucket::SupportedMatch),
            &scoreboard(ScoreBucket::SupportedMismatch),
        );

        assert!(report.should_fail());
        assert_eq!(report.regressions.len(), 1);
    }

    #[test]
    fn baseline_allows_improvement_to_supported_match() {
        let report = compare_scoreboards(
            &scoreboard(ScoreBucket::SkippedUnsupported),
            &scoreboard(ScoreBucket::SupportedMatch),
        );

        assert!(!report.should_fail());
        assert_eq!(report.improvements.len(), 1);
    }
}
