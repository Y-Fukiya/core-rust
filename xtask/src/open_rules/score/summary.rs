use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ExecutionProvenance, ExecutionProvenanceDetail, ScoreBucket, ScoredCase};

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
    #[serde(default)]
    pub deferred_oracle_gap_mismatch: usize,
    #[serde(default)]
    pub deferred_oracle_gap_skipped: usize,
    #[serde(default)]
    pub deferred_oracle_gap_breakdown: DeferredOracleGapBreakdown,
    pub skipped_unsupported: usize,
    #[serde(default)]
    pub mixed_skipped_and_issues: usize,
    #[serde(default)]
    pub no_official_oracle: usize,
    pub harness_error: usize,
    pub supported_accuracy: Option<f64>,
    pub coverage: Option<f64>,
    #[serde(default)]
    pub native_engine_supported_match: usize,
    #[serde(default)]
    pub native_engine_supported_mismatch: usize,
    #[serde(default)]
    pub native_engine_supported_accuracy: Option<f64>,
    #[serde(default)]
    pub native_engine_coverage: Option<f64>,
    #[serde(default)]
    pub rule_id_hand_port_supported_match: usize,
    #[serde(default)]
    pub rule_id_hand_port_supported_mismatch: usize,
    #[serde(default)]
    pub rule_id_hand_port_supported_accuracy: Option<f64>,
    #[serde(default)]
    pub rule_id_hand_port_coverage: Option<f64>,
    #[serde(default)]
    pub unknown_provenance_supported_match: usize,
    #[serde(default)]
    pub unknown_provenance_supported_mismatch: usize,
    #[serde(default)]
    pub unknown_provenance_supported_accuracy: Option<f64>,
    #[serde(default)]
    pub unknown_provenance_coverage: Option<f64>,
    #[serde(default)]
    pub by_execution_provenance_detail: Vec<ExecutionProvenanceDetailSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionProvenanceDetailSummary {
    pub detail: ExecutionProvenanceDetail,
    pub supported_match: usize,
    pub supported_mismatch: usize,
    pub supported_accuracy: Option<f64>,
    pub coverage: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeferredOracleGapBreakdown {
    #[serde(default)]
    pub candidate_skipped: usize,
    #[serde(default)]
    pub official_fixture_gap: usize,
    #[serde(default)]
    pub standard_filter_oracle_gap: usize,
    #[serde(default)]
    pub oracle_identity_gap: usize,
    #[serde(default)]
    pub unverified_semantics_gap: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ScoreGate {
    pub min_coverage: Option<f64>,
    pub max_skipped_unsupported: Option<usize>,
    pub coverage_threshold_failed: bool,
    pub skipped_unsupported_threshold_failed: bool,
    #[serde(default)]
    pub deferred_oracle_gap_failed: bool,
    pub should_fail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupSummary {
    pub name: String,
    pub summary: ScoreSummary,
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
        let deferred_oracle_gap_mismatch =
            *counts.get("deferred_oracle_gap_mismatch").unwrap_or(&0);
        let deferred_oracle_gap_skipped = *counts.get("deferred_oracle_gap_skipped").unwrap_or(&0);
        let deferred_oracle_gap_breakdown = deferred_oracle_gap_breakdown(cases);
        let skipped_unsupported = *counts.get("skipped_unsupported").unwrap_or(&0);
        let mixed_skipped_and_issues = *counts.get("mixed_skipped_and_issues").unwrap_or(&0);
        let no_official_oracle = *counts.get("no_official_oracle").unwrap_or(&0);
        let harness_error = *counts.get("harness_error").unwrap_or(&0);
        let supported = supported_match + supported_mismatch;
        let total_cases = cases.len();
        let native_engine_supported_match = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMatch,
            ExecutionProvenance::NativeEngine,
        );
        let native_engine_supported_mismatch = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMismatch,
            ExecutionProvenance::NativeEngine,
        );
        let rule_id_hand_port_supported_match = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMatch,
            ExecutionProvenance::RuleIdHandPort,
        );
        let rule_id_hand_port_supported_mismatch = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMismatch,
            ExecutionProvenance::RuleIdHandPort,
        );
        let unknown_provenance_supported_match = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMatch,
            ExecutionProvenance::Unknown,
        );
        let unknown_provenance_supported_mismatch = count_supported_by_provenance(
            cases,
            ScoreBucket::SupportedMismatch,
            ExecutionProvenance::Unknown,
        );
        let by_execution_provenance_detail = execution_provenance_detail_summaries(cases);
        Self {
            total_cases,
            supported_match,
            official_oracle_match,
            synthetic_oracle_match,
            unverified_synthetic_oracle_match,
            supported_mismatch,
            deferred_oracle_gap_mismatch,
            deferred_oracle_gap_skipped,
            deferred_oracle_gap_breakdown,
            skipped_unsupported,
            mixed_skipped_and_issues,
            no_official_oracle,
            harness_error,
            supported_accuracy: (supported > 0).then(|| supported_match as f64 / supported as f64),
            coverage: (total_cases > 0).then(|| supported as f64 / total_cases as f64),
            native_engine_supported_match,
            native_engine_supported_mismatch,
            native_engine_supported_accuracy: supported_accuracy_for_counts(
                native_engine_supported_match,
                native_engine_supported_mismatch,
            ),
            native_engine_coverage: coverage_for_counts(
                native_engine_supported_match + native_engine_supported_mismatch,
                total_cases,
            ),
            rule_id_hand_port_supported_match,
            rule_id_hand_port_supported_mismatch,
            rule_id_hand_port_supported_accuracy: supported_accuracy_for_counts(
                rule_id_hand_port_supported_match,
                rule_id_hand_port_supported_mismatch,
            ),
            rule_id_hand_port_coverage: coverage_for_counts(
                rule_id_hand_port_supported_match + rule_id_hand_port_supported_mismatch,
                total_cases,
            ),
            unknown_provenance_supported_match,
            unknown_provenance_supported_mismatch,
            unknown_provenance_supported_accuracy: supported_accuracy_for_counts(
                unknown_provenance_supported_match,
                unknown_provenance_supported_mismatch,
            ),
            unknown_provenance_coverage: coverage_for_counts(
                unknown_provenance_supported_match + unknown_provenance_supported_mismatch,
                total_cases,
            ),
            by_execution_provenance_detail,
        }
    }

    pub fn should_fail(&self) -> bool {
        self.supported_mismatch > 0
            || self.deferred_oracle_gap_mismatch > 0
            || self.skipped_unsupported > 0
            || self.harness_error > 0
            || self.mixed_skipped_and_issues > 0
    }
}

impl ScoreGate {
    pub(super) fn new(
        summary: &ScoreSummary,
        min_coverage: Option<f64>,
        max_skipped_unsupported: Option<usize>,
        fail_on_deferred_oracle_gap: bool,
    ) -> Self {
        let coverage_threshold_failed = coverage_threshold_failed(summary, min_coverage);
        let skipped_unsupported_threshold_failed =
            skipped_unsupported_threshold_failed(summary, max_skipped_unsupported);
        let deferred_oracle_gap_failed = fail_on_deferred_oracle_gap
            && (summary.deferred_oracle_gap_mismatch > 0
                || summary.deferred_oracle_gap_skipped > 0);
        Self {
            min_coverage,
            max_skipped_unsupported,
            coverage_threshold_failed,
            skipped_unsupported_threshold_failed,
            deferred_oracle_gap_failed,
            should_fail: summary.should_fail()
                || coverage_threshold_failed
                || skipped_unsupported_threshold_failed
                || deferred_oracle_gap_failed,
        }
    }
}

pub(super) fn grouped_summary(
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

fn execution_provenance_detail_summaries(
    cases: &[ScoredCase],
) -> Vec<ExecutionProvenanceDetailSummary> {
    [
        ExecutionProvenanceDetail::GenericEngine,
        ExecutionProvenanceDetail::RuleSpecificEngineSemantics,
        ExecutionProvenanceDetail::CompatibilityPolicy,
        ExecutionProvenanceDetail::OracleGapNormalized,
        ExecutionProvenanceDetail::RuleIdHandPort,
        ExecutionProvenanceDetail::Unknown,
    ]
    .into_iter()
    .filter_map(|detail| {
        let supported_match =
            count_supported_by_provenance_detail(cases, ScoreBucket::SupportedMatch, &detail);
        let supported_mismatch =
            count_supported_by_provenance_detail(cases, ScoreBucket::SupportedMismatch, &detail);
        let supported = supported_match + supported_mismatch;
        (supported > 0).then(|| ExecutionProvenanceDetailSummary {
            detail,
            supported_match,
            supported_mismatch,
            supported_accuracy: supported_accuracy_for_counts(supported_match, supported_mismatch),
            coverage: coverage_for_counts(supported, cases.len()),
        })
    })
    .collect()
}

fn count_supported_by_provenance_detail(
    cases: &[ScoredCase],
    bucket: ScoreBucket,
    detail: &ExecutionProvenanceDetail,
) -> usize {
    cases
        .iter()
        .filter(|case| case.bucket == bucket && &case.execution_provenance_detail == detail)
        .count()
}

fn count_supported_by_provenance(
    cases: &[ScoredCase],
    bucket: ScoreBucket,
    provenance: ExecutionProvenance,
) -> usize {
    cases
        .iter()
        .filter(|case| case.bucket == bucket && case.execution_provenance == provenance)
        .count()
}

fn deferred_oracle_gap_breakdown(cases: &[ScoredCase]) -> DeferredOracleGapBreakdown {
    let mut breakdown = DeferredOracleGapBreakdown::default();
    for case in cases
        .iter()
        .filter(|case| case.bucket == ScoreBucket::DeferredOracleGapSkipped)
    {
        match deferred_oracle_gap_breakdown_kind(case) {
            DeferredOracleGapBreakdownKind::OfficialFixtureGap => {
                breakdown.official_fixture_gap += 1;
            }
            DeferredOracleGapBreakdownKind::StandardFilterOracleGap => {
                breakdown.standard_filter_oracle_gap += 1;
            }
            DeferredOracleGapBreakdownKind::CandidateSkipped => {
                breakdown.candidate_skipped += 1;
            }
            DeferredOracleGapBreakdownKind::OracleIdentityGap => {
                breakdown.oracle_identity_gap += 1;
            }
            DeferredOracleGapBreakdownKind::UnverifiedSemanticsGap => {
                breakdown.unverified_semantics_gap += 1;
            }
        }
    }
    breakdown
}

enum DeferredOracleGapBreakdownKind {
    CandidateSkipped,
    OfficialFixtureGap,
    StandardFilterOracleGap,
    OracleIdentityGap,
    UnverifiedSemanticsGap,
}

fn deferred_oracle_gap_breakdown_kind(case: &ScoredCase) -> DeferredOracleGapBreakdownKind {
    let reason = case.reason.as_deref().unwrap_or_default();
    if reason.contains("official oracle fixture gap") {
        DeferredOracleGapBreakdownKind::OfficialFixtureGap
    } else if reason.contains("standard applicability oracle semantics") {
        DeferredOracleGapBreakdownKind::StandardFilterOracleGap
    } else if !case.skipped_reasons.is_empty() {
        DeferredOracleGapBreakdownKind::CandidateSkipped
    } else if case.missing_count.unwrap_or(0) > 0 || case.extra_count.unwrap_or(0) > 0 {
        DeferredOracleGapBreakdownKind::OracleIdentityGap
    } else {
        DeferredOracleGapBreakdownKind::UnverifiedSemanticsGap
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

fn supported_accuracy_for_counts(matches: usize, mismatches: usize) -> Option<f64> {
    let supported = matches + mismatches;
    (supported > 0).then(|| matches as f64 / supported as f64)
}

fn coverage_for_counts(supported: usize, total_cases: usize) -> Option<f64> {
    (total_cases > 0).then(|| supported as f64 / total_cases as f64)
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
