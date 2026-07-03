use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::score::{ScoreSummary, Scoreboard, ScoredCase};

#[derive(Debug, Clone, Parser)]
pub struct ScoreDeltaArgs {
    #[arg(long, value_name = "FILE")]
    pub default_scoreboard: PathBuf,

    #[arg(long, value_name = "FILE")]
    pub strict_scoreboard: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ScoreboardDelta {
    default_scoreboard: PathBuf,
    strict_scoreboard: PathBuf,
    counts: Vec<CountDelta>,
    ratios: Vec<RatioDelta>,
    deferred_oracle_gap_breakdown: Vec<CountDelta>,
    execution_provenance_detail: Vec<CountDelta>,
    scoring_policy: Vec<CountDelta>,
    scoring_normalizations: Vec<CountDelta>,
    bucket_transitions: Vec<TransitionSummary>,
    normalization_transitions: Vec<TransitionSummary>,
    top_affected_rules: Vec<RuleImpactSummary>,
    example_changed_cases: Vec<ChangedCaseExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CountDelta {
    metric: String,
    default: usize,
    strict: usize,
    delta: isize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RatioDelta {
    metric: String,
    default: Option<f64>,
    strict: Option<f64>,
    delta_percentage_points: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct CaseKey {
    scope: String,
    rule_id: String,
    case_kind: String,
    case_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TransitionSummary {
    transition: String,
    cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuleImpactSummary {
    rule_id: String,
    cases_affected: usize,
    main_transition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ChangedCaseExample {
    rule_id: String,
    case_kind: String,
    case_id: String,
    transition: String,
    scoring_normalizations: Vec<String>,
}

pub fn run(args: ScoreDeltaArgs) -> Result<bool> {
    let default = read_scoreboard(&args.default_scoreboard)?;
    let strict = read_scoreboard(&args.strict_scoreboard)?;
    validate_scoreboard_pair(&default, &strict)?;
    let delta = ScoreboardDelta::new(
        args.default_scoreboard.clone(),
        args.strict_scoreboard.clone(),
        &default,
        &strict,
    );
    write_delta(&args.out, &delta)?;
    Ok(false)
}

fn read_scoreboard(path: &Path) -> Result<Scoreboard> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("parse {}", path.display()))
}

fn write_delta(out_dir: &Path, delta: &ScoreboardDelta) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let json_path = out_dir.join("scoreboard-delta.json");
    let json_file =
        File::create(&json_path).with_context(|| format!("create {}", json_path.display()))?;
    serde_json::to_writer_pretty(json_file, delta)
        .with_context(|| format!("write {}", json_path.display()))?;

    let markdown_path = out_dir.join("summary.md");
    let mut markdown = File::create(&markdown_path)
        .with_context(|| format!("create {}", markdown_path.display()))?;
    markdown
        .write_all(markdown_delta(delta).as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))?;
    Ok(())
}

impl ScoreboardDelta {
    fn new(
        default_scoreboard: PathBuf,
        strict_scoreboard: PathBuf,
        default: &Scoreboard,
        strict: &Scoreboard,
    ) -> Self {
        Self {
            default_scoreboard,
            strict_scoreboard,
            counts: count_deltas(&default.summary, &strict.summary),
            ratios: ratio_deltas(&default.summary, &strict.summary),
            deferred_oracle_gap_breakdown: deferred_oracle_gap_breakdown_deltas(
                &default.summary,
                &strict.summary,
            ),
            execution_provenance_detail: map_deltas(
                execution_provenance_detail_counts(&default.summary),
                execution_provenance_detail_counts(&strict.summary),
            ),
            scoring_policy: map_deltas(
                scoring_policy_counts(&default.summary),
                scoring_policy_counts(&strict.summary),
            ),
            scoring_normalizations: map_deltas(
                scoring_normalization_counts(&default.summary),
                scoring_normalization_counts(&strict.summary),
            ),
            bucket_transitions: bucket_transitions(default, strict),
            normalization_transitions: normalization_transitions(default, strict),
            top_affected_rules: top_affected_rules(default, strict, 20),
            example_changed_cases: example_changed_cases(default, strict, 20),
        }
    }
}

fn validate_scoreboard_pair(default: &Scoreboard, strict: &Scoreboard) -> Result<()> {
    if default.upstream.repo != strict.upstream.repo {
        bail!(
            "scoreboards use different upstream repos: default={} strict={}",
            default.upstream.repo,
            strict.upstream.repo
        );
    }
    if default.upstream.expected_sha != strict.upstream.expected_sha {
        bail!(
            "scoreboards use different expected upstream SHAs: default={:?} strict={:?}",
            default.upstream.expected_sha,
            strict.upstream.expected_sha
        );
    }
    if default.upstream.observed_sha != strict.upstream.observed_sha {
        bail!(
            "scoreboards use different observed upstream SHAs: default={:?} strict={:?}",
            default.upstream.observed_sha,
            strict.upstream.observed_sha
        );
    }
    if default.summary.total_cases != strict.summary.total_cases {
        bail!(
            "scoreboards have different total case counts: default={} strict={}",
            default.summary.total_cases,
            strict.summary.total_cases
        );
    }

    reject_duplicate_case_keys("default", &default.cases)?;
    reject_duplicate_case_keys("strict", &strict.cases)?;

    let default_keys = case_key_set(&default.cases);
    let strict_keys = case_key_set(&strict.cases);
    if default_keys != strict_keys {
        let missing_from_strict = default_keys
            .difference(&strict_keys)
            .take(5)
            .map(case_key_label)
            .collect::<Vec<_>>();
        let extra_in_strict = strict_keys
            .difference(&default_keys)
            .take(5)
            .map(case_key_label)
            .collect::<Vec<_>>();
        bail!(
            "scoreboards have different case key sets: missing_from_strict={missing_from_strict:?} extra_in_strict={extra_in_strict:?}"
        );
    }
    Ok(())
}

fn reject_duplicate_case_keys(label: &str, cases: &[ScoredCase]) -> Result<()> {
    let mut seen = BTreeSet::<CaseKey>::new();
    let mut duplicates = BTreeSet::<CaseKey>::new();
    for case in cases {
        let key = case_key(case);
        if !seen.insert(key.clone()) {
            duplicates.insert(key);
        }
    }
    if !duplicates.is_empty() {
        let examples = duplicates
            .iter()
            .take(5)
            .map(case_key_label)
            .collect::<Vec<_>>();
        bail!("{label} scoreboard has duplicate case keys: {examples:?}");
    }
    Ok(())
}

fn count_deltas(default: &ScoreSummary, strict: &ScoreSummary) -> Vec<CountDelta> {
    [
        (
            "supported_match",
            default.supported_match,
            strict.supported_match,
        ),
        (
            "supported_mismatch",
            default.supported_mismatch,
            strict.supported_mismatch,
        ),
        (
            "deferred_oracle_gap_mismatch",
            default.deferred_oracle_gap_mismatch,
            strict.deferred_oracle_gap_mismatch,
        ),
        (
            "deferred_oracle_gap_skipped",
            default.deferred_oracle_gap_skipped,
            strict.deferred_oracle_gap_skipped,
        ),
        (
            "skipped_unsupported",
            default.skipped_unsupported,
            strict.skipped_unsupported,
        ),
        (
            "mixed_skipped_and_issues",
            default.mixed_skipped_and_issues,
            strict.mixed_skipped_and_issues,
        ),
        (
            "no_official_oracle",
            default.no_official_oracle,
            strict.no_official_oracle,
        ),
        ("harness_error", default.harness_error, strict.harness_error),
        (
            "native_engine_supported_match",
            default.native_engine_supported_match,
            strict.native_engine_supported_match,
        ),
        (
            "rule_id_hand_port_supported_match",
            default.rule_id_hand_port_supported_match,
            strict.rule_id_hand_port_supported_match,
        ),
    ]
    .into_iter()
    .map(|(metric, default, strict)| CountDelta::new(metric, default, strict))
    .collect()
}

fn ratio_deltas(default: &ScoreSummary, strict: &ScoreSummary) -> Vec<RatioDelta> {
    [
        (
            "supported_accuracy",
            default.supported_accuracy,
            strict.supported_accuracy,
        ),
        ("coverage", default.coverage, strict.coverage),
        (
            "native_engine_coverage",
            default.native_engine_coverage,
            strict.native_engine_coverage,
        ),
        (
            "rule_id_hand_port_coverage",
            default.rule_id_hand_port_coverage,
            strict.rule_id_hand_port_coverage,
        ),
    ]
    .into_iter()
    .map(|(metric, default, strict)| RatioDelta {
        metric: metric.to_owned(),
        default,
        strict,
        delta_percentage_points: default.zip(strict).map(|(default, strict)| {
            let delta = strict - default;
            delta * 100.0
        }),
    })
    .collect()
}

fn deferred_oracle_gap_breakdown_deltas(
    default: &ScoreSummary,
    strict: &ScoreSummary,
) -> Vec<CountDelta> {
    [
        (
            "candidate_skipped",
            default.deferred_oracle_gap_breakdown.candidate_skipped,
            strict.deferred_oracle_gap_breakdown.candidate_skipped,
        ),
        (
            "official_fixture_gap",
            default.deferred_oracle_gap_breakdown.official_fixture_gap,
            strict.deferred_oracle_gap_breakdown.official_fixture_gap,
        ),
        (
            "standard_filter_oracle_gap",
            default
                .deferred_oracle_gap_breakdown
                .standard_filter_oracle_gap,
            strict
                .deferred_oracle_gap_breakdown
                .standard_filter_oracle_gap,
        ),
        (
            "oracle_identity_gap",
            default.deferred_oracle_gap_breakdown.oracle_identity_gap,
            strict.deferred_oracle_gap_breakdown.oracle_identity_gap,
        ),
        (
            "unverified_semantics_gap",
            default
                .deferred_oracle_gap_breakdown
                .unverified_semantics_gap,
            strict
                .deferred_oracle_gap_breakdown
                .unverified_semantics_gap,
        ),
    ]
    .into_iter()
    .map(|(metric, default, strict)| CountDelta::new(metric, default, strict))
    .collect()
}

fn bucket_transitions(default: &Scoreboard, strict: &Scoreboard) -> Vec<TransitionSummary> {
    let default_cases = cases_by_key(&default.cases);
    let strict_cases = cases_by_key(&strict.cases);
    let mut counts = BTreeMap::<String, usize>::new();
    for (key, default_case) in default_cases {
        let strict_case = strict_cases
            .get(&key)
            .expect("case key set is validated before delta generation");
        if default_case.bucket != strict_case.bucket {
            *counts
                .entry(bucket_transition(default_case, strict_case))
                .or_default() += 1;
        }
    }
    transition_summaries(counts)
}

fn normalization_transitions(default: &Scoreboard, strict: &Scoreboard) -> Vec<TransitionSummary> {
    let default_cases = cases_by_key(&default.cases);
    let strict_cases = cases_by_key(&strict.cases);
    let mut counts = BTreeMap::<String, usize>::new();
    for (key, default_case) in default_cases {
        if default_case.scoring_normalizations.is_empty() {
            continue;
        }
        let strict_case = strict_cases
            .get(&key)
            .expect("case key set is validated before delta generation");
        let transition = bucket_transition(default_case, strict_case);
        for normalization in &default_case.scoring_normalizations {
            *counts
                .entry(format!("{normalization}: {transition}"))
                .or_default() += 1;
        }
    }
    transition_summaries(counts)
}

fn top_affected_rules(
    default: &Scoreboard,
    strict: &Scoreboard,
    limit: usize,
) -> Vec<RuleImpactSummary> {
    let default_cases = cases_by_key(&default.cases);
    let strict_cases = cases_by_key(&strict.cases);
    let mut by_rule = BTreeMap::<String, Vec<String>>::new();
    for (key, default_case) in default_cases {
        let strict_case = strict_cases
            .get(&key)
            .expect("case key set is validated before delta generation");
        if cases_differ_for_delta(default_case, strict_case) {
            by_rule
                .entry(default_case.rule_id.clone())
                .or_default()
                .push(case_transition_label(default_case, strict_case));
        }
    }
    let mut summaries = by_rule
        .into_iter()
        .map(|(rule_id, transitions)| {
            let cases_affected = transitions.len();
            let main_transition =
                most_common_transition(transitions).unwrap_or_else(|| "unknown".to_owned());
            RuleImpactSummary {
                rule_id,
                cases_affected,
                main_transition,
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .cases_affected
            .cmp(&left.cases_affected)
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    summaries.truncate(limit);
    summaries
}

fn example_changed_cases(
    default: &Scoreboard,
    strict: &Scoreboard,
    limit: usize,
) -> Vec<ChangedCaseExample> {
    let default_cases = cases_by_key(&default.cases);
    let strict_cases = cases_by_key(&strict.cases);
    let mut examples = default_cases
        .into_iter()
        .filter_map(|(key, default_case)| {
            let strict_case = strict_cases
                .get(&key)
                .expect("case key set is validated before delta generation");
            if !cases_differ_for_delta(default_case, strict_case) {
                return None;
            }
            Some(ChangedCaseExample {
                rule_id: default_case.rule_id.clone(),
                case_kind: default_case.case_kind.clone(),
                case_id: default_case.case_id.clone(),
                transition: case_transition_label(default_case, strict_case),
                scoring_normalizations: default_case.scoring_normalizations.clone(),
            })
        })
        .collect::<Vec<_>>();
    examples.sort_by(|left, right| {
        left.rule_id
            .cmp(&right.rule_id)
            .then_with(|| left.case_kind.cmp(&right.case_kind))
            .then_with(|| left.case_id.cmp(&right.case_id))
    });
    examples.truncate(limit);
    examples
}

fn cases_by_key(cases: &[ScoredCase]) -> BTreeMap<CaseKey, &ScoredCase> {
    cases.iter().map(|case| (case_key(case), case)).collect()
}

fn case_key_set(cases: &[ScoredCase]) -> BTreeSet<CaseKey> {
    cases.iter().map(case_key).collect()
}

fn case_key(case: &ScoredCase) -> CaseKey {
    CaseKey {
        scope: case.scope.clone(),
        rule_id: case.rule_id.clone(),
        case_kind: case.case_kind.clone(),
        case_id: case.case_id.clone(),
    }
}

fn case_key_label(key: &CaseKey) -> String {
    format!(
        "{}/{}/{}/{}",
        key.scope, key.rule_id, key.case_kind, key.case_id
    )
}

fn bucket_transition(default_case: &ScoredCase, strict_case: &ScoredCase) -> String {
    format!(
        "{} -> {}",
        default_case.bucket.as_str(),
        strict_case.bucket.as_str()
    )
}

fn case_transition_label(default_case: &ScoredCase, strict_case: &ScoredCase) -> String {
    if default_case.bucket != strict_case.bucket {
        bucket_transition(default_case, strict_case)
    } else if default_case.scoring_policy != strict_case.scoring_policy {
        format!(
            "scoring_policy {} -> {}",
            default_case.scoring_policy.as_str(),
            strict_case.scoring_policy.as_str()
        )
    } else if default_case.scoring_normalizations != strict_case.scoring_normalizations {
        "scoring_normalizations changed".to_owned()
    } else if default_case.official_issue_count != strict_case.official_issue_count
        || default_case.candidate_issue_count != strict_case.candidate_issue_count
        || default_case.missing_count != strict_case.missing_count
        || default_case.extra_count != strict_case.extra_count
        || default_case.issue_fingerprint_hash != strict_case.issue_fingerprint_hash
    {
        "issue details changed".to_owned()
    } else {
        "metadata changed".to_owned()
    }
}

fn cases_differ_for_delta(default_case: &ScoredCase, strict_case: &ScoredCase) -> bool {
    default_case.bucket != strict_case.bucket
        || default_case.scoring_policy != strict_case.scoring_policy
        || default_case.scoring_normalizations != strict_case.scoring_normalizations
        || default_case.official_issue_count != strict_case.official_issue_count
        || default_case.candidate_issue_count != strict_case.candidate_issue_count
        || default_case.missing_count != strict_case.missing_count
        || default_case.extra_count != strict_case.extra_count
        || default_case.issue_fingerprint_hash != strict_case.issue_fingerprint_hash
}

fn transition_summaries(counts: BTreeMap<String, usize>) -> Vec<TransitionSummary> {
    let mut summaries = counts
        .into_iter()
        .map(|(transition, cases)| TransitionSummary { transition, cases })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| left.transition.cmp(&right.transition))
    });
    summaries
}

fn most_common_transition(transitions: Vec<String>) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for transition in transitions {
        *counts.entry(transition).or_default() += 1;
    }
    transition_summaries(counts)
        .into_iter()
        .next()
        .map(|summary| summary.transition)
}

fn execution_provenance_detail_counts(summary: &ScoreSummary) -> BTreeMap<String, usize> {
    summary
        .by_execution_provenance_detail
        .iter()
        .map(|entry| {
            (
                entry.detail.as_str().to_owned(),
                entry.supported_match + entry.supported_mismatch,
            )
        })
        .collect()
}

fn scoring_policy_counts(summary: &ScoreSummary) -> BTreeMap<String, usize> {
    summary
        .by_scoring_policy
        .iter()
        .map(|entry| {
            (
                entry.policy.as_str().to_owned(),
                entry.supported_match + entry.supported_mismatch,
            )
        })
        .collect()
}

fn scoring_normalization_counts(summary: &ScoreSummary) -> BTreeMap<String, usize> {
    summary
        .scoring_normalization_counts
        .iter()
        .map(|entry| (entry.normalization.clone(), entry.cases))
        .collect()
}

fn map_deltas(
    default: BTreeMap<String, usize>,
    strict: BTreeMap<String, usize>,
) -> Vec<CountDelta> {
    let mut keys = default
        .keys()
        .chain(strict.keys())
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|metric| {
            let default = default.get(&metric).copied().unwrap_or_default();
            let strict = strict.get(&metric).copied().unwrap_or_default();
            CountDelta::new(metric, default, strict)
        })
        .collect()
}

impl CountDelta {
    fn new(metric: impl Into<String>, default: usize, strict: usize) -> Self {
        Self {
            metric: metric.into(),
            default,
            strict,
            delta: strict as isize - default as isize,
        }
    }
}

fn markdown_delta(delta: &ScoreboardDelta) -> String {
    let mut lines = vec![
        "# Open Rules Default-vs-Strict Scoring Delta".to_owned(),
        String::new(),
        format!("- Default scoreboard: `{}`", delta.default_scoreboard.display()),
        format!("- Strict scoreboard: `{}`", delta.strict_scoreboard.display()),
        String::new(),
        "Strict scoring disables oracle-gap reclassification and oracle-informed score normalizations. Deltas are `strict - default`, so positive supported mismatches or negative coverage indicate compatibility policy influence."
            .to_owned(),
        String::new(),
    ];
    push_count_table(&mut lines, "Bucket And Provenance Counts", &delta.counts);
    push_ratio_table(&mut lines, "Coverage And Accuracy", &delta.ratios);
    push_count_table(
        &mut lines,
        "Deferred Oracle-Gap Breakdown",
        &delta.deferred_oracle_gap_breakdown,
    );
    push_count_table(
        &mut lines,
        "Execution Provenance Detail Counts",
        &delta.execution_provenance_detail,
    );
    push_count_table(&mut lines, "Scoring Policy Counts", &delta.scoring_policy);
    lines.push(
        "Scoring Policy counts supported match/mismatch cases. Scoring Normalization Counts include all scored cases, including deferred cases."
            .to_owned(),
    );
    lines.push(String::new());
    push_count_table(
        &mut lines,
        "Scoring Normalization Counts",
        &delta.scoring_normalizations,
    );
    push_transition_table(&mut lines, "Bucket Transitions", &delta.bucket_transitions);
    push_transition_table(
        &mut lines,
        "Normalization-Affected Transitions",
        &delta.normalization_transitions,
    );
    push_rule_impact_table(&mut lines, "Top Affected Rules", &delta.top_affected_rules);
    push_changed_case_examples(
        &mut lines,
        "Example Changed Cases",
        &delta.example_changed_cases,
    );
    lines.join("\n") + "\n"
}

fn push_count_table(lines: &mut Vec<String>, title: &str, rows: &[CountDelta]) {
    if rows.is_empty() {
        return;
    }
    lines.extend([
        format!("## {title}"),
        String::new(),
        "| Metric | Default | Strict | Delta |".to_owned(),
        "|---|---:|---:|---:|".to_owned(),
    ]);
    for row in rows {
        lines.push(format!(
            "| `{}` | {} | {} | {:+} |",
            row.metric, row.default, row.strict, row.delta
        ));
    }
    lines.push(String::new());
}

fn push_ratio_table(lines: &mut Vec<String>, title: &str, rows: &[RatioDelta]) {
    if rows.is_empty() {
        return;
    }
    lines.extend([
        format!("## {title}"),
        String::new(),
        "| Metric | Default | Strict | Delta pp |".to_owned(),
        "|---|---:|---:|---:|".to_owned(),
    ]);
    for row in rows {
        lines.push(format!(
            "| `{}` | {} | {} | {} |",
            row.metric,
            percent_or_na(row.default),
            percent_or_na(row.strict),
            signed_percentage_points_or_na(row.delta_percentage_points)
        ));
    }
    lines.push(String::new());
}

fn push_transition_table(lines: &mut Vec<String>, title: &str, rows: &[TransitionSummary]) {
    if rows.is_empty() {
        return;
    }
    lines.extend([
        format!("## {title}"),
        String::new(),
        "| Transition | Cases |".to_owned(),
        "|---|---:|".to_owned(),
    ]);
    for row in rows {
        lines.push(format!("| `{}` | {} |", row.transition, row.cases));
    }
    lines.push(String::new());
}

fn push_rule_impact_table(lines: &mut Vec<String>, title: &str, rows: &[RuleImpactSummary]) {
    if rows.is_empty() {
        return;
    }
    lines.extend([
        format!("## {title}"),
        String::new(),
        "| Rule ID | Cases affected | Main transition |".to_owned(),
        "|---|---:|---|".to_owned(),
    ]);
    for row in rows {
        lines.push(format!(
            "| `{}` | {} | `{}` |",
            row.rule_id, row.cases_affected, row.main_transition
        ));
    }
    lines.push(String::new());
}

fn push_changed_case_examples(lines: &mut Vec<String>, title: &str, rows: &[ChangedCaseExample]) {
    if rows.is_empty() {
        return;
    }
    lines.extend([
        format!("## {title}"),
        String::new(),
        "| Rule ID | Kind | Case | Transition | Default normalizations |".to_owned(),
        "|---|---|---|---|---|".to_owned(),
    ]);
    for row in rows {
        let normalizations = if row.scoring_normalizations.is_empty() {
            "none".to_owned()
        } else {
            row.scoring_normalizations.join(", ")
        };
        lines.push(format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` |",
            row.rule_id, row.case_kind, row.case_id, row.transition, normalizations
        ));
    }
    lines.push(String::new());
}

fn percent_or_na(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.2}%", value * 100.0))
        .unwrap_or_else(|| "n/a".to_owned())
}

fn signed_percentage_points_or_na(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:+.2} pp"))
        .unwrap_or_else(|| "n/a".to_owned())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::open_rules::score::{
        ExecutionProvenance, ExecutionProvenanceDetail, ScoreBucket, ScoredCase, ScoringPolicy,
    };
    use crate::open_rules::upstream::UpstreamInfo;

    #[test]
    fn writes_default_vs_strict_delta_summary() {
        let dir = tempdir().expect("tempdir");
        let default_path = dir.path().join("default.json");
        let strict_path = dir.path().join("strict.json");
        let out = dir.path().join("delta");

        let default = scoreboard(vec![
            case(
                "CORE-000001",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
            case(
                "CORE-000002",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::OracleGapNormalized,
                vec!["row_locator_identity_relaxed"],
            ),
        ]);
        let strict = scoreboard(vec![
            case(
                "CORE-000001",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
            case(
                "CORE-000002",
                ScoreBucket::SupportedMismatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
        ]);
        serde_json::to_writer(
            File::create(&default_path).expect("default scoreboard"),
            &default,
        )
        .expect("write default");
        serde_json::to_writer(
            File::create(&strict_path).expect("strict scoreboard"),
            &strict,
        )
        .expect("write strict");

        let should_fail = run(ScoreDeltaArgs {
            default_scoreboard: default_path,
            strict_scoreboard: strict_path,
            out: out.clone(),
        })
        .expect("run delta");

        assert!(!should_fail);
        let markdown = fs::read_to_string(out.join("summary.md")).expect("read markdown");
        assert!(markdown.contains("# Open Rules Default-vs-Strict Scoring Delta"));
        assert!(markdown.contains("| `supported_match` | 2 | 1 | -1 |"));
        assert!(markdown.contains("| `supported_mismatch` | 0 | 1 | +1 |"));
        assert!(markdown.contains("| `coverage` | 100.00% | 100.00% | +0.00 pp |"));
        assert!(markdown.contains("| `oracle_gap_normalized` | 1 | 0 | -1 |"));
        assert!(markdown.contains("## Deferred Oracle-Gap Breakdown"));
        assert!(markdown.contains("Scoring Policy counts supported match/mismatch cases."));
        assert!(
            markdown.contains("| `supported_match -> supported_mismatch` | 1 |"),
            "{markdown}"
        );
        assert!(markdown.contains(
            "| `row_locator_identity_relaxed: supported_match -> supported_mismatch` | 1 |"
        ));
        assert!(
            markdown.contains("| `CORE-000002` | 1 | `supported_match -> supported_mismatch` |"),
            "{markdown}"
        );
        assert!(
            markdown.contains(
                "| `CORE-000002` | `negative` | `01` | `supported_match -> supported_mismatch` | `row_locator_identity_relaxed` |"
            ),
            "{markdown}"
        );

        let json = fs::read_to_string(out.join("scoreboard-delta.json")).expect("read delta json");
        assert!(json.contains("\"metric\": \"supported_mismatch\""));
        assert!(json.contains("\"example_changed_cases\""));
    }

    #[test]
    fn rejects_scoreboards_with_different_case_sets() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let strict = scoreboard(vec![case(
            "CORE-000002",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);

        let error = validate_scoreboard_pair(&default, &strict).expect_err("case set mismatch");

        assert!(
            error.to_string().contains("different case key sets"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_scoreboards_with_different_upstream_repo() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let mut strict = default.clone();
        strict.upstream.repo = "https://example.invalid/corpus.git".to_owned();

        let error = validate_scoreboard_pair(&default, &strict).expect_err("repo mismatch");

        assert!(
            error.to_string().contains("different upstream repos"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_scoreboards_with_different_expected_sha() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let mut strict = default.clone();
        strict.upstream.expected_sha = Some("different".to_owned());

        let error = validate_scoreboard_pair(&default, &strict).expect_err("expected sha mismatch");

        assert!(
            error
                .to_string()
                .contains("different expected upstream SHAs"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_scoreboards_with_different_observed_sha() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let mut strict = default.clone();
        strict.upstream.observed_sha = Some("different".to_owned());

        let error = validate_scoreboard_pair(&default, &strict).expect_err("observed sha mismatch");

        assert!(
            error
                .to_string()
                .contains("different observed upstream SHAs"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_scoreboards_with_different_total_cases() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let mut strict = default.clone();
        strict.summary.total_cases += 1;

        let error = validate_scoreboard_pair(&default, &strict).expect_err("total cases mismatch");

        assert!(
            error.to_string().contains("different total case counts"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_scoreboards_with_duplicate_case_keys() {
        let default = scoreboard(vec![
            case(
                "CORE-000001",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
            case(
                "CORE-000001",
                ScoreBucket::SupportedMismatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
        ]);
        let strict = scoreboard(vec![
            case(
                "CORE-000001",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
            case(
                "CORE-000001",
                ScoreBucket::SupportedMatch,
                ScoringPolicy::StrictIdentity,
                vec![],
            ),
        ]);

        let error = validate_scoreboard_pair(&default, &strict).expect_err("duplicate key");

        assert!(
            error.to_string().contains("duplicate case keys"),
            "{error:?}"
        );
    }

    #[test]
    fn reports_case_level_issue_detail_changes() {
        let default = scoreboard(vec![case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        )]);
        let mut strict_case = case(
            "CORE-000001",
            ScoreBucket::SupportedMatch,
            ScoringPolicy::StrictIdentity,
            vec![],
        );
        strict_case.candidate_issue_count = Some(2);
        strict_case.extra_count = Some(1);
        strict_case.issue_fingerprint_hash = Some("changed".to_owned());
        let strict = scoreboard(vec![strict_case]);

        let delta = ScoreboardDelta::new(
            "default.json".into(),
            "strict.json".into(),
            &default,
            &strict,
        );
        let markdown = markdown_delta(&delta);

        assert!(
            markdown.contains("| `CORE-000001` | 1 | `issue details changed` |"),
            "{markdown}"
        );
        assert!(
            markdown.contains(
                "| `CORE-000001` | `negative` | `01` | `issue details changed` | `none` |"
            ),
            "{markdown}"
        );
    }

    fn scoreboard(cases: Vec<ScoredCase>) -> Scoreboard {
        Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("observed".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: Vec::new(),
            },
            cases,
        )
    }

    fn case(
        rule_id: &str,
        bucket: ScoreBucket,
        scoring_policy: ScoringPolicy,
        scoring_normalizations: Vec<&str>,
    ) -> ScoredCase {
        let execution_provenance_detail = if scoring_normalizations.is_empty() {
            ExecutionProvenanceDetail::GenericEngine
        } else {
            ExecutionProvenanceDetail::OracleGapNormalized
        };
        ScoredCase {
            scope: "Published".to_owned(),
            rule_id: rule_id.to_owned(),
            case_kind: "negative".to_owned(),
            case_id: "01".to_owned(),
            case_dir: "case".into(),
            official_results_csv: "official.csv".into(),
            candidate_report_csv: "report.csv".into(),
            execution_provenance: ExecutionProvenance::NativeEngine,
            execution_provenance_detail,
            scoring_policy,
            bucket,
            reason: None,
            skipped_reasons: Vec::new(),
            scoring_normalizations: scoring_normalizations
                .into_iter()
                .map(str::to_owned)
                .collect(),
            official_issue_count: Some(1),
            candidate_issue_count: Some(1),
            missing_count: Some(0),
            extra_count: Some(0),
            issue_fingerprint_hash: Some(crate::open_rules::score::issue_fingerprint_hash(
                &[],
                &[],
            )),
            missing: Vec::new(),
            extra: Vec::new(),
        }
    }
}
