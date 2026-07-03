use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::score::{ScoreSummary, Scoreboard};

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
    execution_provenance_detail: Vec<CountDelta>,
    scoring_policy: Vec<CountDelta>,
    scoring_normalizations: Vec<CountDelta>,
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

pub fn run(args: ScoreDeltaArgs) -> Result<bool> {
    let default = read_scoreboard(&args.default_scoreboard)?;
    let strict = read_scoreboard(&args.strict_scoreboard)?;
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
        }
    }
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
        "Execution Provenance Detail Counts",
        &delta.execution_provenance_detail,
    );
    push_count_table(&mut lines, "Scoring Policy Counts", &delta.scoring_policy);
    push_count_table(
        &mut lines,
        "Scoring Normalization Counts",
        &delta.scoring_normalizations,
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

        let json = fs::read_to_string(out.join("scoreboard-delta.json")).expect("read delta json");
        assert!(json.contains("\"metric\": \"supported_mismatch\""));
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
