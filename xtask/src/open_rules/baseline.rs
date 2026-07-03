//! Open Rules baseline comparison.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::score::{
    execution_provenance_detail_for_case, issue_fingerprint_hash,
    scoring_policy_for_normalizations, ExecutionProvenance, ExecutionProvenanceDetail, ScoreBucket,
    Scoreboard, ScoredCase, ScoringPolicy,
};

#[derive(Debug, Clone, Parser)]
pub struct BaselineArgs {
    #[arg(long, value_name = "FILE")]
    pub scoreboard: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub baseline: PathBuf,
}

#[derive(Debug, Clone, Parser)]
pub struct CanonicalizeBaselineArgs {
    #[arg(long, value_name = "FILE")]
    pub scoreboard: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub out: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineReport {
    pub regressions: Vec<BaselineDifference>,
    pub improvements: Vec<BaselineDifference>,
    #[serde(default)]
    pub review_required: Vec<BaselineDifference>,
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
    #[serde(default)]
    pub baseline_execution_provenance_detail: Option<ExecutionProvenanceDetail>,
    #[serde(default)]
    pub current_execution_provenance_detail: Option<ExecutionProvenanceDetail>,
    #[serde(default)]
    pub baseline_scoring_policy: Option<ScoringPolicy>,
    #[serde(default)]
    pub current_scoring_policy: Option<ScoringPolicy>,
    #[serde(default)]
    pub baseline_official_issue_count: Option<usize>,
    #[serde(default)]
    pub current_official_issue_count: Option<usize>,
    #[serde(default)]
    pub baseline_candidate_issue_count: Option<usize>,
    #[serde(default)]
    pub current_candidate_issue_count: Option<usize>,
    #[serde(default)]
    pub baseline_missing_count: Option<usize>,
    #[serde(default)]
    pub current_missing_count: Option<usize>,
    #[serde(default)]
    pub baseline_extra_count: Option<usize>,
    #[serde(default)]
    pub current_extra_count: Option<usize>,
    #[serde(default)]
    pub baseline_issue_fingerprint: Option<String>,
    #[serde(default)]
    pub current_issue_fingerprint: Option<String>,
    #[serde(default)]
    pub baseline_scoring_normalizations: Vec<String>,
    #[serde(default)]
    pub current_scoring_normalizations: Vec<String>,
    pub message: String,
}

pub fn run(args: BaselineArgs) -> Result<bool> {
    let baseline = read_scoreboard(&args.baseline)?;
    let current = read_scoreboard(&args.scoreboard)?;
    let report = compare_scoreboards(&baseline, &current);
    println!(
        "open-rules baseline: {} regression(s), {} improvement(s), {} review-required",
        report.regressions.len(),
        report.improvements.len(),
        report.review_required.len(),
    );
    for regression in &report.regressions {
        println!(
            "regression {}: {} -> {} ({})",
            regression.case_key,
            bucket_and_provenance(
                &regression.baseline_bucket,
                &regression.baseline_execution_provenance,
                &regression.baseline_execution_provenance_detail,
                &regression.baseline_scoring_policy,
            ),
            bucket_and_provenance(
                &regression.current_bucket,
                &regression.current_execution_provenance,
                &regression.current_execution_provenance_detail,
                &regression.current_scoring_policy,
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
                &improvement.baseline_execution_provenance_detail,
                &improvement.baseline_scoring_policy,
            ),
            bucket_and_provenance(
                &improvement.current_bucket,
                &improvement.current_execution_provenance,
                &improvement.current_execution_provenance_detail,
                &improvement.current_scoring_policy,
            )
        );
    }
    for review in &report.review_required {
        println!(
            "review-required {}: {} -> {} ({})",
            review.case_key,
            bucket_and_provenance(
                &review.baseline_bucket,
                &review.baseline_execution_provenance,
                &review.baseline_execution_provenance_detail,
                &review.baseline_scoring_policy,
            ),
            bucket_and_provenance(
                &review.current_bucket,
                &review.current_execution_provenance,
                &review.current_execution_provenance_detail,
                &review.current_scoring_policy,
            ),
            review.message
        );
    }
    Ok(report.should_fail())
}

pub fn canonicalize(args: CanonicalizeBaselineArgs) -> Result<bool> {
    let scoreboard = read_scoreboard(&args.scoreboard)?;
    let canonicalized = canonicalize_scoreboard_for_baseline(scoreboard);
    let mut file =
        File::create(&args.out).with_context(|| format!("create {}", args.out.display()))?;
    serde_json::to_writer_pretty(&mut file, &canonicalized)
        .with_context(|| format!("write {}", args.out.display()))?;
    writeln!(file).with_context(|| format!("write {}", args.out.display()))?;
    println!("wrote canonical open-rules baseline {}", args.out.display());
    Ok(false)
}

fn canonicalize_scoreboard_for_baseline(mut scoreboard: Scoreboard) -> Scoreboard {
    scoreboard.upstream.lock_path = PathBuf::from("tests/open_rules/upstream.lock");
    scoreboard.upstream.warnings = scoreboard
        .upstream
        .warnings
        .into_iter()
        .map(canonicalize_reason_text)
        .collect();
    for case in &mut scoreboard.cases {
        case.case_dir = canonicalize_scoreboard_path(&case.case_dir);
        case.official_results_csv = canonicalize_scoreboard_path(&case.official_results_csv);
        case.candidate_report_csv = canonicalize_scoreboard_path(&case.candidate_report_csv);
        case.reason = case.reason.take().map(canonicalize_reason_text);
        case.scoring_policy = scoring_policy_for_normalizations(&case.scoring_normalizations);
        case.execution_provenance_detail = execution_provenance_detail_for_case(
            &case.rule_id,
            &case.execution_provenance,
            &case.scoring_normalizations,
        );
        case.missing_count = Some(case_missing_count(case));
        case.extra_count = Some(case_extra_count(case));
        case.issue_fingerprint_hash = Some(case_issue_fingerprint_hash(case));
        case.missing.clear();
        case.extra.clear();
    }
    Scoreboard::new_with_gate(
        scoreboard.upstream,
        scoreboard.cases,
        scoreboard.gate.min_coverage,
        scoreboard.gate.max_skipped_unsupported,
        false,
    )
}

fn canonicalize_reason_text(reason: String) -> String {
    reason
        .split_whitespace()
        .map(|token| {
            let trimmed = token.trim_end_matches([';', ',', ')', ':']);
            let suffix = &token[trimmed.len()..];
            if let Some(path) = canonicalize_reason_path(trimmed) {
                format!("{}{}", path.display(), suffix)
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn canonicalize_reason_path(token: &str) -> Option<PathBuf> {
    let path = Path::new(token);
    if !path.is_absolute() {
        let canonicalized = canonicalize_scoreboard_path(path);
        return (canonicalized != path).then_some(canonicalized);
    }
    let canonicalized = canonicalize_scoreboard_path(path);
    if canonicalized.is_absolute() {
        Some(PathBuf::from("<absolute-path>"))
    } else {
        Some(canonicalized)
    }
}

fn canonicalize_scoreboard_path(path: &Path) -> PathBuf {
    let components = path.components().collect::<Vec<_>>();
    if let Some(index) = components
        .iter()
        .position(|component| component.as_os_str() == "target")
    {
        if components
            .get(index + 1)
            .and_then(|component| component.as_os_str().to_str())
            .is_some_and(|component| component.starts_with("open-rules-core-rs-"))
        {
            let mut path = PathBuf::from("target");
            path.push("<core-rs-results-root>");
            for component in &components[index + 2..] {
                path.push(component.as_os_str());
            }
            return path;
        }
        return components_to_path(&components[index..]);
    }
    if let Some(index) = components
        .iter()
        .position(|component| component.as_os_str() == "Published")
    {
        return components_to_path(&components[index..]);
    }
    path.to_path_buf()
}

fn components_to_path(components: &[Component<'_>]) -> PathBuf {
    components
        .iter()
        .fold(PathBuf::new(), |mut path, component| {
            path.push(component.as_os_str());
            path
        })
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
    let mut review_required = Vec::new();
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
                if scoring_normalizations_require_review(baseline_case, current_case) {
                    review_required.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "scoring normalizations require review",
                    ));
                } else if is_improvement(&baseline_case.bucket, &current_case.bucket) {
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
                } else if provenance_requires_review(baseline_case, current_case) {
                    review_required.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "execution provenance requires review",
                    ));
                } else if provenance_detail_requires_review(baseline_case, current_case) {
                    review_required.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "execution provenance detail requires review",
                    ));
                } else if bucket_transition_requires_review(baseline_case, current_case) {
                    review_required.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "case bucket requires review",
                    ));
                } else if provenance_regressed(baseline_case, current_case) {
                    regressions.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "execution provenance regressed",
                    ));
                } else if same_bucket_issue_details_regressed(baseline_case, current_case) {
                    regressions.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "case issue details regressed within the same bucket",
                    ));
                } else if same_bucket_issue_fingerprints_changed(baseline_case, current_case) {
                    regressions.push(difference(
                        key,
                        Some(baseline_case),
                        Some(current_case),
                        "case issue fingerprints changed within the same bucket",
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
                } else if !current_case.scoring_normalizations.is_empty() {
                    review_required.push(difference(
                        key,
                        None,
                        Some(current_case),
                        "new case uses scoring normalizations",
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
    if current.summary.deferred_oracle_gap_mismatch > baseline.summary.deferred_oracle_gap_mismatch
    {
        review_required.push(difference(
            "summary/deferred_oracle_gap_mismatch".to_owned(),
            None,
            None,
            "deferred oracle-gap mismatch count increased",
        ));
    }
    if current.summary.deferred_oracle_gap_skipped > baseline.summary.deferred_oracle_gap_skipped {
        review_required.push(difference(
            "summary/deferred_oracle_gap_skipped".to_owned(),
            None,
            None,
            "deferred oracle-gap skipped count increased",
        ));
    }
    if current.summary.no_official_oracle > baseline.summary.no_official_oracle {
        review_required.push(difference(
            "summary/no_official_oracle".to_owned(),
            None,
            None,
            "no official oracle count increased",
        ));
    }
    if scoring_normalization_counts_increased(baseline, current) {
        review_required.push(difference(
            "summary/scoring_normalization_counts".to_owned(),
            None,
            None,
            "scoring normalization count increased",
        ));
    }

    BaselineReport {
        regressions,
        improvements,
        review_required,
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
        !self.regressions.is_empty() || !self.review_required.is_empty()
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
            | ScoreBucket::DeferredOracleGapMismatch
            | ScoreBucket::SkippedUnsupported
            | ScoreBucket::HarnessError
            | ScoreBucket::MixedSkippedAndIssues
    )
}

fn provenance_requires_review(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    supported_match_provenance_changed(baseline, current)
        && baseline.execution_provenance == ExecutionProvenance::RuleIdHandPort
        && current.execution_provenance == ExecutionProvenance::NativeEngine
}

fn provenance_detail_requires_review(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    if baseline.bucket != ScoreBucket::SupportedMatch
        || current.bucket != ScoreBucket::SupportedMatch
        || baseline.execution_provenance_detail == current.execution_provenance_detail
    {
        return false;
    }
    !matches!(
        (
            &baseline.execution_provenance_detail,
            &current.execution_provenance_detail
        ),
        (
            ExecutionProvenanceDetail::Unknown,
            ExecutionProvenanceDetail::GenericEngine
                | ExecutionProvenanceDetail::RuleSpecificEngineSemantics
                | ExecutionProvenanceDetail::CompatibilityPolicy
                | ExecutionProvenanceDetail::RuleIdHandPort
        )
    )
}

fn bucket_transition_requires_review(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    baseline.bucket == ScoreBucket::SupportedMismatch
        && current.bucket == ScoreBucket::DeferredOracleGapMismatch
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

fn scoring_normalizations_require_review(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    normalized_scoring_normalizations(&baseline.scoring_normalizations)
        != normalized_scoring_normalizations(&current.scoring_normalizations)
}

fn normalized_scoring_normalizations(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

fn scoring_normalization_counts_increased(baseline: &Scoreboard, current: &Scoreboard) -> bool {
    let baseline_counts = baseline
        .summary
        .scoring_normalization_counts
        .iter()
        .map(|entry| (entry.normalization.as_str(), entry.cases))
        .collect::<BTreeMap<_, _>>();
    current
        .summary
        .scoring_normalization_counts
        .iter()
        .any(|entry| {
            entry.cases
                > *baseline_counts
                    .get(entry.normalization.as_str())
                    .unwrap_or(&0)
        })
}

fn same_bucket_issue_details_regressed(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    if baseline.bucket != current.bucket || baseline.bucket == ScoreBucket::SupportedMatch {
        return false;
    }
    if case_missing_count(current) > case_missing_count(baseline)
        || case_extra_count(current) > case_extra_count(baseline)
    {
        return true;
    }
    issue_count_distance(current) > issue_count_distance(baseline)
}

fn issue_count_distance(case: &ScoredCase) -> usize {
    match (case.official_issue_count, case.candidate_issue_count) {
        (Some(official), Some(candidate)) => official.abs_diff(candidate),
        _ => 0,
    }
}

fn same_bucket_issue_fingerprints_changed(baseline: &ScoredCase, current: &ScoredCase) -> bool {
    if baseline.bucket != current.bucket || baseline.bucket == ScoreBucket::SupportedMatch {
        return false;
    }
    if case_missing_count(baseline) != case_missing_count(current)
        || case_extra_count(baseline) != case_extra_count(current)
        || issue_count_distance(baseline) != issue_count_distance(current)
    {
        return false;
    }
    case_issue_fingerprint_hash(baseline) != case_issue_fingerprint_hash(current)
}

fn case_missing_count(case: &ScoredCase) -> usize {
    case.missing_count.unwrap_or(case.missing.len())
}

fn case_extra_count(case: &ScoredCase) -> usize {
    case.extra_count.unwrap_or(case.extra.len())
}

fn case_issue_fingerprint_hash(case: &ScoredCase) -> String {
    case.issue_fingerprint_hash
        .clone()
        .unwrap_or_else(|| issue_fingerprint_hash(&case.missing, &case.extra))
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
        baseline_execution_provenance_detail: baseline_case
            .map(|case| case.execution_provenance_detail.clone()),
        current_execution_provenance_detail: current_case
            .map(|case| case.execution_provenance_detail.clone()),
        baseline_scoring_policy: baseline_case.map(|case| case.scoring_policy.clone()),
        current_scoring_policy: current_case.map(|case| case.scoring_policy.clone()),
        baseline_official_issue_count: baseline_case.and_then(|case| case.official_issue_count),
        current_official_issue_count: current_case.and_then(|case| case.official_issue_count),
        baseline_candidate_issue_count: baseline_case.and_then(|case| case.candidate_issue_count),
        current_candidate_issue_count: current_case.and_then(|case| case.candidate_issue_count),
        baseline_missing_count: baseline_case.map(case_missing_count),
        current_missing_count: current_case.map(case_missing_count),
        baseline_extra_count: baseline_case.map(case_extra_count),
        current_extra_count: current_case.map(case_extra_count),
        baseline_issue_fingerprint: baseline_case.map(case_issue_fingerprint_hash),
        current_issue_fingerprint: current_case.map(case_issue_fingerprint_hash),
        baseline_scoring_normalizations: baseline_case
            .map(|case| normalized_scoring_normalizations(&case.scoring_normalizations))
            .unwrap_or_default(),
        current_scoring_normalizations: current_case
            .map(|case| normalized_scoring_normalizations(&case.scoring_normalizations))
            .unwrap_or_default(),
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
    detail: &Option<ExecutionProvenanceDetail>,
    scoring_policy: &Option<ScoringPolicy>,
) -> String {
    let bucket = bucket_name(bucket);
    let base = match (provenance, detail) {
        (Some(provenance), Some(detail)) => {
            format!(
                "{bucket}/{}/{}",
                provenance_name(provenance),
                detail.as_str()
            )
        }
        (Some(provenance), None) => format!("{bucket}/{}", provenance_name(provenance)),
        (None, Some(detail)) => format!("{bucket}/{}", detail.as_str()),
        (None, None) => bucket.to_owned(),
    };
    match scoring_policy {
        Some(scoring_policy) => format!("{base}/{}", scoring_policy.as_str()),
        None => base,
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
    use crate::open_rules::normalize::IssueKey;
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
                execution_provenance_detail:
                    crate::open_rules::score::ExecutionProvenanceDetail::Unknown,
                scoring_policy: ScoringPolicy::StrictIdentity,
                bucket,
                reason: None,
                skipped_reasons: Vec::new(),
                scoring_normalizations: Vec::new(),
                official_issue_count: Some(1),
                candidate_issue_count: Some(1),
                missing_count: Some(0),
                extra_count: Some(0),
                issue_fingerprint_hash: Some(issue_fingerprint_hash(&[], &[])),
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

    fn scoreboard_with_provenance_detail(
        bucket: ScoreBucket,
        execution_provenance_detail: ExecutionProvenanceDetail,
    ) -> Scoreboard {
        let mut scoreboard = scoreboard(bucket);
        scoreboard.cases[0].execution_provenance = ExecutionProvenance::NativeEngine;
        scoreboard.cases[0].execution_provenance_detail = execution_provenance_detail;
        scoreboard.summary = crate::open_rules::score::ScoreSummary::from_cases(&scoreboard.cases);
        scoreboard
    }

    fn issue(row: &str) -> IssueKey {
        IssueKey {
            rule_id: "CORE-OPEN-0001".to_owned(),
            dataset: "AE".to_owned(),
            domain: "AE".to_owned(),
            row: row.to_owned(),
            variables: vec!["AESEQ".to_owned()],
            usubjid: String::new(),
            seq: String::new(),
        }
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
        assert!(report.review_required.is_empty());
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
    fn baseline_fails_when_supported_mismatch_becomes_deferred_oracle_gap() {
        let report = compare_scoreboards(
            &scoreboard(ScoreBucket::SupportedMismatch),
            &scoreboard(ScoreBucket::DeferredOracleGapMismatch),
        );

        assert!(report.should_fail());
        assert!(report.regressions.iter().any(|regression| {
            regression.case_key == "summary/coverage" && regression.message == "coverage regressed"
        }));
        assert!(report.regressions.iter().any(|regression| {
            regression.case_key == "Published/CORE-OPEN-0001/negative/01"
                && regression.message == "case bucket regressed"
                && regression.baseline_bucket == Some(ScoreBucket::SupportedMismatch)
                && regression.current_bucket == Some(ScoreBucket::DeferredOracleGapMismatch)
        }));
    }

    #[test]
    fn baseline_fails_when_deferred_oracle_gap_mismatch_increases() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.deferred_oracle_gap_mismatch =
            baseline.summary.deferred_oracle_gap_mismatch + 1;

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.case_key == "summary/deferred_oracle_gap_mismatch"
                && review.message == "deferred oracle-gap mismatch count increased"
        }));
    }

    #[test]
    fn baseline_fails_when_deferred_oracle_gap_skipped_increases() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.deferred_oracle_gap_skipped =
            baseline.summary.deferred_oracle_gap_skipped + 1;

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.case_key == "summary/deferred_oracle_gap_skipped"
                && review.message == "deferred oracle-gap skipped count increased"
        }));
    }

    #[test]
    fn baseline_fails_when_no_official_oracle_increases() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.no_official_oracle = baseline.summary.no_official_oracle + 1;

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.case_key == "summary/no_official_oracle"
                && review.message == "no official oracle count increased"
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
    fn baseline_fails_hand_port_to_native_as_review_required() {
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

        assert!(report.should_fail());
        assert!(report.improvements.is_empty());
        assert!(report
            .review_required
            .iter()
            .any(|review| review.message == "execution provenance requires review"));
        let review = report
            .review_required
            .iter()
            .find(|review| review.message == "execution provenance requires review")
            .expect("provenance review");
        assert_eq!(
            review.baseline_execution_provenance,
            Some(ExecutionProvenance::RuleIdHandPort)
        );
        assert_eq!(
            review.current_execution_provenance,
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
        assert!(report.review_required.is_empty());
    }

    #[test]
    fn baseline_requires_review_when_supported_match_normalizations_change() {
        let mut baseline = scoreboard(ScoreBucket::SupportedMatch);
        baseline.cases[0].scoring_normalizations = vec!["row_locator_identity_relaxed".to_owned()];
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.cases[0].scoring_normalizations = vec![
            "row_locator_identity_relaxed".to_owned(),
            "output_context_variable_aligned".to_owned(),
        ];

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        let review = report
            .review_required
            .iter()
            .find(|review| review.message == "scoring normalizations require review")
            .expect("normalization review");
        assert_eq!(
            review.baseline_scoring_normalizations,
            vec!["row_locator_identity_relaxed".to_owned()]
        );
        assert_eq!(
            review.current_scoring_normalizations,
            vec![
                "output_context_variable_aligned".to_owned(),
                "row_locator_identity_relaxed".to_owned()
            ]
        );
    }

    #[test]
    fn baseline_requires_review_when_deferred_normalizations_change() {
        let mut baseline = scoreboard(ScoreBucket::DeferredOracleGapSkipped);
        baseline.cases[0].scoring_normalizations = vec!["row_locator_identity_relaxed".to_owned()];
        let mut current = scoreboard(ScoreBucket::DeferredOracleGapSkipped);
        current.cases[0].scoring_normalizations = vec![
            "row_locator_identity_relaxed".to_owned(),
            "output_context_variable_aligned".to_owned(),
        ];

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.message == "scoring normalizations require review"
                && review.baseline_bucket == Some(ScoreBucket::DeferredOracleGapSkipped)
                && review.current_bucket == Some(ScoreBucket::DeferredOracleGapSkipped)
        }));
    }

    #[test]
    fn baseline_requires_review_when_new_case_uses_normalization() {
        let baseline = Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("expected".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: Vec::new(),
            },
            Vec::new(),
        );
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.cases[0].scoring_normalizations =
            vec!["output_context_variable_aligned".to_owned()];
        current.summary = crate::open_rules::score::ScoreSummary::from_cases(&current.cases);

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.message == "new case uses scoring normalizations"
                && review.case_key == "Published/CORE-OPEN-0001/negative/01"
        }));
    }

    #[test]
    fn baseline_requires_review_when_summary_normalization_count_increases() {
        let baseline = scoreboard(ScoreBucket::SupportedMatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.summary.scoring_normalization_counts =
            vec![crate::open_rules::score::ScoringNormalizationSummary {
                normalization: "output_context_variable_aligned".to_owned(),
                cases: 1,
            }];

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.case_key == "summary/scoring_normalization_counts"
                && review.message == "scoring normalization count increased"
        }));
    }

    #[test]
    fn baseline_requires_review_when_improvement_uses_normalization() {
        let baseline = scoreboard(ScoreBucket::SupportedMismatch);
        let mut current = scoreboard(ScoreBucket::SupportedMatch);
        current.cases[0].scoring_normalizations = vec!["row_locator_identity_relaxed".to_owned()];

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.improvements.is_empty());
        assert!(report
            .review_required
            .iter()
            .any(|review| review.message == "scoring normalizations require review"));
    }

    #[test]
    fn baseline_fails_when_supported_match_detail_requires_review() {
        let report = compare_scoreboards(
            &scoreboard_with_provenance_detail(
                ScoreBucket::SupportedMatch,
                ExecutionProvenanceDetail::GenericEngine,
            ),
            &scoreboard_with_provenance_detail(
                ScoreBucket::SupportedMatch,
                ExecutionProvenanceDetail::OracleGapNormalized,
            ),
        );

        assert!(report.should_fail());
        assert!(report.review_required.iter().any(|review| {
            review.message == "execution provenance detail requires review"
                && review.baseline_execution_provenance_detail
                    == Some(ExecutionProvenanceDetail::GenericEngine)
                && review.current_execution_provenance_detail
                    == Some(ExecutionProvenanceDetail::OracleGapNormalized)
        }));
    }

    #[test]
    fn canonicalized_baseline_uses_portable_paths_and_recomputes_summary() {
        let mut scoreboard = scoreboard(ScoreBucket::SupportedMatch);
        scoreboard.cases[0].case_dir =
            "/private/tmp/cdisc-open-rules/Published/CORE-OPEN-0001/negative/01".into();
        scoreboard.cases[0].official_results_csv =
            "/private/tmp/cdisc-open-rules/Published/CORE-OPEN-0001/negative/01/results/results.csv"
                .into();
        scoreboard.cases[0].candidate_report_csv =
            "/Users/example/core-rust/target/open-rules-core-rs-upstream/Published/CORE-OPEN-0001/negative/01/report.csv"
                .into();
        scoreboard.upstream.lock_path =
            "/Users/example/core-rust/xtask/../tests/open_rules/upstream.lock".into();
        scoreboard.upstream.warnings = vec![
            "git rev-parse failed for /private/tmp/cdisc-open-rules: not a git checkout".to_owned(),
        ];
        scoreboard.cases[0].reason = Some(
            "official results.csv is malformed: CSV contains merge conflict markers: /private/tmp/cdisc-open-rules/Published/CORE-OPEN-0001/negative/01/results/results.csv; and ../cdisc-open-rules/Published/CORE-OPEN-0001/negative/01/results/results.csv; excluded"
                .to_owned(),
        );
        scoreboard.cases[0].missing = vec![issue("1")];
        scoreboard.cases[0].extra = vec![issue("2")];
        scoreboard.cases[0].missing_count = None;
        scoreboard.cases[0].extra_count = None;
        scoreboard.cases[0].issue_fingerprint_hash = None;
        scoreboard.summary.total_cases = 99;

        let canonicalized = canonicalize_scoreboard_for_baseline(scoreboard);

        assert_eq!(
            canonicalized.cases[0].case_dir,
            PathBuf::from("Published/CORE-OPEN-0001/negative/01")
        );
        assert_eq!(
            canonicalized.cases[0].official_results_csv,
            PathBuf::from("Published/CORE-OPEN-0001/negative/01/results/results.csv")
        );
        assert_eq!(
            canonicalized.cases[0].candidate_report_csv,
            PathBuf::from(
                "target/<core-rs-results-root>/Published/CORE-OPEN-0001/negative/01/report.csv"
            )
        );
        assert_eq!(
            canonicalized.upstream.lock_path,
            PathBuf::from("tests/open_rules/upstream.lock")
        );
        assert_eq!(
            canonicalized.upstream.warnings,
            vec!["git rev-parse failed for <absolute-path>: not a git checkout"]
        );
        assert_eq!(
            canonicalized.cases[0].reason.as_deref(),
            Some(
                "official results.csv is malformed: CSV contains merge conflict markers: Published/CORE-OPEN-0001/negative/01/results/results.csv; and Published/CORE-OPEN-0001/negative/01/results/results.csv; excluded"
            )
        );
        assert!(canonicalized.cases[0].missing.is_empty());
        assert!(canonicalized.cases[0].extra.is_empty());
        assert_eq!(canonicalized.cases[0].missing_count, Some(1));
        assert_eq!(canonicalized.cases[0].extra_count, Some(1));
        assert!(canonicalized.cases[0].issue_fingerprint_hash.is_some());
        assert_eq!(canonicalized.summary.total_cases, 1);
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

    #[test]
    fn baseline_fails_when_supported_mismatch_issue_details_worsen() {
        let mut baseline = scoreboard(ScoreBucket::SupportedMismatch);
        baseline.cases[0].official_issue_count = Some(2);
        baseline.cases[0].candidate_issue_count = Some(1);
        baseline.cases[0].missing = vec![issue("2")];
        baseline.cases[0].missing_count = Some(1);
        baseline.cases[0].extra_count = Some(0);
        baseline.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(
            &baseline.cases[0].missing,
            &baseline.cases[0].extra,
        ));

        let mut current = baseline.clone();
        current.cases[0].candidate_issue_count = Some(0);
        current.cases[0].missing = vec![issue("1"), issue("2")];
        current.cases[0].missing_count = Some(2);
        current.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(
            &current.cases[0].missing,
            &current.cases[0].extra,
        ));

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        let regression = report
            .regressions
            .iter()
            .find(|regression| {
                regression.message == "case issue details regressed within the same bucket"
            })
            .expect("same-bucket issue regression");
        assert_eq!(regression.baseline_missing_count, Some(1));
        assert_eq!(regression.current_missing_count, Some(2));
        assert_eq!(regression.baseline_candidate_issue_count, Some(1));
        assert_eq!(regression.current_candidate_issue_count, Some(0));
    }

    #[test]
    fn baseline_fails_when_same_bucket_issue_fingerprint_changes() {
        let mut baseline = scoreboard(ScoreBucket::SupportedMismatch);
        baseline.cases[0].missing = vec![issue("1")];
        baseline.cases[0].missing_count = Some(1);
        baseline.cases[0].extra_count = Some(0);
        baseline.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(
            &baseline.cases[0].missing,
            &baseline.cases[0].extra,
        ));

        let mut current = baseline.clone();
        current.cases[0].missing = vec![issue("2")];
        current.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(
            &current.cases[0].missing,
            &current.cases[0].extra,
        ));

        let report = compare_scoreboards(&baseline, &current);

        assert!(report.should_fail());
        assert!(report.regressions.iter().any(|regression| {
            regression.message == "case issue fingerprints changed within the same bucket"
        }));
    }

    #[test]
    fn stripped_baseline_uses_portable_issue_counts_and_hash() {
        let missing = vec![issue("1")];
        let mut baseline = scoreboard(ScoreBucket::SupportedMismatch);
        baseline.cases[0].official_issue_count = Some(1);
        baseline.cases[0].candidate_issue_count = Some(0);
        baseline.cases[0].missing.clear();
        baseline.cases[0].extra.clear();
        baseline.cases[0].missing_count = Some(1);
        baseline.cases[0].extra_count = Some(0);
        baseline.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(&missing, &[]));

        let mut current = baseline.clone();
        current.cases[0].missing = missing;
        current.cases[0].missing_count = Some(1);
        current.cases[0].extra_count = Some(0);
        current.cases[0].issue_fingerprint_hash = Some(issue_fingerprint_hash(
            &current.cases[0].missing,
            &current.cases[0].extra,
        ));

        let report = compare_scoreboards(&baseline, &current);

        assert!(!report.should_fail());
        assert!(report.regressions.is_empty());
    }

    #[test]
    fn stripped_baseline_json_deserializes_without_issue_arrays() {
        let mut value =
            serde_json::to_value(scoreboard(ScoreBucket::SupportedMismatch)).expect("serialize");
        let case = value
            .get_mut("cases")
            .and_then(|cases| cases.as_array_mut())
            .and_then(|cases| cases.first_mut())
            .and_then(|case| case.as_object_mut())
            .expect("first case object");
        case.remove("missing");
        case.remove("extra");

        let scoreboard: Scoreboard = serde_json::from_value(value).expect("deserialize");

        assert!(scoreboard.cases[0].missing.is_empty());
        assert!(scoreboard.cases[0].extra.is_empty());
    }
}
