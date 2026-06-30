//! Open Rules baseline comparison.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::score::{ExecutionProvenance, ScoreBucket, Scoreboard, ScoredCase};

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
    #[serde(default)]
    pub baseline_execution_provenance: Option<ExecutionProvenance>,
    #[serde(default)]
    pub current_execution_provenance: Option<ExecutionProvenance>,
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
            bucket_and_provenance(
                &regression.baseline_bucket,
                &regression.baseline_execution_provenance,
            ),
            bucket_and_provenance(
                &regression.current_bucket,
                &regression.current_execution_provenance,
            ),
            regression.message
        );
    }
    for improvement in &report.improvements {
        println!(
            "improvement {}: {} -> {}",
            improvement.case_key,
            bucket_and_provenance(
                &improvement.baseline_bucket,
                &improvement.baseline_execution_provenance,
            ),
            bucket_and_provenance(
                &improvement.current_bucket,
                &improvement.current_execution_provenance,
            )
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
                        Some(baseline_case),
                        Some(current_case),
                        "case improved to supported_match",
                    ));
                } else if is_regression(&baseline_case.bucket, &current_case.bucket) {
                    regressions.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "case bucket regressed",
                    ));
                } else if provenance_improved(baseline_case, current_case) {
                    improvements.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "execution provenance improved",
                    ));
                } else if provenance_regressed(baseline_case, current_case) {
                    regressions.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "execution provenance regressed",
                    ));
                }
            }
            (Some(baseline_case), None) => regressions.push(difference(
                key,
                Some(baseline_case),
                None,
                "baseline case is missing from current scoreboard",
            )),
            (None, Some(current_case)) => {
                if is_failing_new_bucket(&current_case.bucket) {
                    regressions.push(difference(
                        key,
                        None,
                        Some(current_case),
                        "new failing case appeared",
                    ));
                }
            }
            (None, None) => {}
        }
    }

    if coverage_regressed(baseline, current) {
        regressions.push(difference(
            "summary/coverage".to_owned(),
            None,
            None,
            "coverage regressed",
        ));
    }
    if native_engine_coverage_regressed(baseline, current) {
        regressions.push(difference(
            "summary/native_engine_coverage".to_owned(),
            None,
            None,
            "native_engine_coverage regressed",
        ));
    }
    if current.summary.skipped_unsupported > baseline.summary.skipped_unsupported {
        regressions.push(difference(
            "summary/skipped_unsupported".to_owned(),
            None,
            None,
            "skipped_unsupported increased",
        ));
    }

    BaselineReport {
        regressions,
        improvements,
    }
}

fn coverage_regressed(baseline: &Scoreboard, current: &Scoreboard) -> bool {
    match (baseline.summary.coverage, current.summary.coverage) {
        (Some(baseline), Some(current)) => current < baseline,
        (Some(_), None) => true,
        _ => false,
    }
}

fn native_engine_coverage_regressed(baseline: &Scoreboard, current: &Scoreboard) -> bool {
    match (
        baseline.summary.native_engine_coverage,
        current.summary.native_engine_coverage,
    ) {
        (Some(baseline), Some(current)) => current < baseline,
        (Some(_), None) => true,
        _ => false,
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
    if matches!(
        baseline,
        ScoreBucket::SupportedMatch | ScoreBucket::SupportedMismatch
    ) && *current == ScoreBucket::SkippedUnsupported
    {
        return true;
    }
    if *baseline == ScoreBucket::SupportedMatch {
        return true;
    }
    is_failing_new_bucket(current)
}

fn is_failing_new_bucket(bucket: &ScoreBucket) -> bool {
    matches!(
        bucket,
        ScoreBucket::SupportedMismatch
            | ScoreBucket::HarnessError
            | ScoreBucket::NoOfficialOracle
            | ScoreBucket::MixedSkippedAndIssues
    )
}

fn provenance_improved(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    supported_match_provenance_changed(baseline, current)
        && baseline.execution_provenance == ExecutionProvenance::RuleIdHandPort
        && current.execution_provenance == ExecutionProvenance::NativeEngine
}

fn provenance_regressed(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    supported_match_provenance_changed(baseline, current)
        && baseline.execution_provenance == ExecutionProvenance::NativeEngine
        && current.execution_provenance != ExecutionProvenance::NativeEngine
}

fn supported_match_provenance_changed(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    if baseline.bucket != ScoreBucket::SupportedMatch
        || current.bucket != ScoreBucket::SupportedMatch
    {
        return false;
    }
    baseline.execution_provenance != current.execution_provenance
}

fn difference(
    case_key: String,
    baseline_case: Option<&ScoredCase>,
    current_case: Option<&ScoredCase>,
    message: &str,
) -> BaselineDifference {
    BaselineDifference {
        case_key,
        baseline_bucket: baseline_case.map(|case| case.bucket.clone()),
        current_bucket: current_case.map(|case| case.bucket.clone()),
        baseline_execution_provenance: baseline_case.map(|case| case.execution_provenance.clone()),
        current_execution_provenance: current_case.map(|case| case.execution_provenance.clone()),
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

fn bucket_and_provenance(
    bucket: &Option<ScoreBucket>,
    provenance: &Option<ExecutionProvenance>,
) -> String {
    let bucket = bucket_name(bucket);
    match provenance {
        Some(provenance) => format!("{bucket}/{}", provenance_name(provenance)),
        None => bucket.to_owned(),
    }
}

fn provenance_name(provenance: &ExecutionProvenance) -> &'static str {
    match provenance {
        ExecutionProvenance::NativeEngine => "native_engine",
        ExecutionProvenance::RuleIdHandPort => "rule_id_hand_port",
        ExecutionProvenance::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
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
                execution_provenance: ExecutionProvenance::Unknown,
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

    fn scoreboard_with_provenance(
        bucket: ScoreBucket,
        execution_provenance: ExecutionProvenance,
    ) -> Scoreboard {
        let mut scoreboard = scoreboard(bucket);
        scoreboard.cases[0].execution_provenance = execution_provenance;
        scoreboard.summary = crate::open_rules::score::ScoreSummary::from_cases(&scoreboard.cases);
        scoreboard
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
    fn baseline_fails_when_supported_case_becomes_skipped() {
        let report = compare_scoreboards(
            &scoreboard(ScoreBucket::SupportedMismatch),
            &scoreboard(ScoreBucket::SkippedUnsupported),
        );

        assert!(report.should_fail());
        assert!(report.regressions.iter().any(|regression| {
            regression.case_key == "Published/CORE-OPEN-0001/negative/01"
                && regression.message == "case bucket regressed"
        }));
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

    #[test]
    fn baseline_fails_when_native_supported_match_becomes_hand_port() {
        let report = compare_scoreboards(
            &scoreboard_with_provenance(
                ScoreBucket::SupportedMatch,
                ExecutionProvenance::NativeEngine,
            ),
            &scoreboard_with_provenance(
                ScoreBucket::SupportedMatch,
                ExecutionProvenance::RuleIdHandPort,
            ),
        );

        assert!(report.should_fail());
        assert!(report
            .regressions
            .iter()
            .any(|regression| regression.message == "execution provenance regressed"));
        let regression = report
            .regressions
            .iter()
            .find(|regression| regression.message == "execution provenance regressed")
            .expect("provenance regression");
        assert_eq!(
            regression.baseline_execution_provenance,
            Some(ExecutionProvenance::NativeEngine)
        );
        assert_eq!(
            regression.current_execution_provenance,
            Some(ExecutionProvenance::RuleIdHandPort)
        );
    }

    #[test]
    fn baseline_reports_hand_port_to_native_as_improvement() {
        let report = compare_scoreboards(
            &scoreboard_with_provenance(
                ScoreBucket::SupportedMatch,
                ExecutionProvenance::RuleIdHandPort,
            ),
            &scoreboard_with_provenance(
                ScoreBucket::SupportedMatch,
                ExecutionProvenance::NativeEngine,
            ),
        );

        assert!(!report.should_fail());
        assert!(report
            .improvements
            .iter()
            .any(|improvement| improvement.message == "execution provenance improved"));
        let improvement = report
            .improvements
            .iter()
            .find(|improvement| improvement.message == "execution provenance improved")
            .expect("provenance improvement");
        assert_eq!(
            improvement.baseline_execution_provenance,
            Some(ExecutionProvenance::RuleIdHandPort)
        );
        assert_eq!(
            improvement.current_execution_provenance,
            Some(ExecutionProvenance::NativeEngine)
        );
    }

    #[test]
    fn baseline_does_not_report_unknown_to_native_as_improvement() {
        let report = compare_scoreboards(
            &scoreboard_with_provenance(ScoreBucket::SupportedMatch, ExecutionProvenance::Unknown),
            &scoreboard_with_provenance(
                ScoreBucket::SupportedMatch,
                ExecutionProvenance::NativeEngine,
            ),
        );

        assert!(!report.should_fail());
        assert!(report.improvements.is_empty());
        assert!(report.regressions.is_empty());
    }

    #[test]
    fn baseline_fails_when_coverage_regresses() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.coverage = Some(0.5);

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report
            .regressions
            .iter()
            .any(|regression| regression.case_key == "summary/coverage"));
    }

    #[test]
    fn baseline_fails_when_native_engine_coverage_regresses() {
        let baseline = scoreboard_with_provenance(
            ScoreBucket::SupportedMatch,
            ExecutionProvenance::NativeEngine,
        );
        let current =
            scoreboard_with_provenance(ScoreBucket::SupportedMatch, ExecutionProvenance::Unknown);

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.regressions.iter().any(|regression| {
            regression.case_key == "summary/native_engine_coverage"
                && regression.message == "native_engine_coverage regressed"
        }));
    }

    #[test]
    fn baseline_fails_when_skipped_unsupported_increases() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.skipped_unsupported = baseline.summary.skipped_unsupported + 1;

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report
            .regressions
            .iter()
            .any(|regression| regression.case_key == "summary/skipped_unsupported"));
    }
}
