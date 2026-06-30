//! Scoreboard JSON and Markdown report writing.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::open_rules::score::{ExecutionProvenance, ScoreBucket, Scoreboard};

pub fn write_scoreboard(out_dir: &Path, scoreboard: &Scoreboard) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let json_path = out_dir.join("scoreboard.json");
    let json_file =
        File::create(&json_path).with_context(|| format!("create {}", json_path.display()))?;
    serde_json::to_writer_pretty(json_file, scoreboard)
        .with_context(|| format!("write {}", json_path.display()))?;

    let markdown_path = out_dir.join("summary.md");
    let mut markdown = File::create(&markdown_path)
        .with_context(|| format!("create {}", markdown_path.display()))?;
    markdown
        .write_all(markdown_summary(scoreboard).as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))?;

    Ok(())
}

fn markdown_summary(scoreboard: &Scoreboard) -> String {
    let summary = &scoreboard.summary;
    let mut lines = vec![
        "# CDISC Open Rules Oracle Compatibility".to_owned(),
        String::new(),
        "| Metric | Value |".to_owned(),
        "|---|---:|".to_owned(),
        format!("| Total cases | {} |", summary.total_cases),
        format!("| Supported match | {} |", summary.supported_match),
        format!(
            "| Official oracle match | {} |",
            summary.official_oracle_match
        ),
        format!(
            "| Synthetic oracle match | {} |",
            summary.synthetic_oracle_match
        ),
        format!(
            "| Unverified synthetic oracle match | {} |",
            summary.unverified_synthetic_oracle_match
        ),
        format!("| Supported mismatch | {} |", summary.supported_mismatch),
        format!("| Skipped unsupported | {} |", summary.skipped_unsupported),
        format!(
            "| Mixed skipped and issues | {} |",
            summary.mixed_skipped_and_issues
        ),
        format!("| No official oracle | {} |", summary.no_official_oracle),
        format!("| Harness error | {} |", summary.harness_error),
        format!(
            "| Supported accuracy | {} |",
            percent_or_na(summary.supported_accuracy)
        ),
        format!("| Coverage | {} |", percent_or_na(summary.coverage)),
        format!(
            "| Native engine coverage | {} |",
            percent_or_na(summary.native_engine_coverage)
        ),
        format!(
            "| Rule-id hand-port coverage | {} |",
            percent_or_na(summary.rule_id_hand_port_coverage)
        ),
        String::new(),
        "## Execution Provenance".to_owned(),
        String::new(),
        "| Provenance | Supported match | Supported mismatch | Accuracy | Coverage |".to_owned(),
        "|---|---:|---:|---:|---:|".to_owned(),
        provenance_row(
            "Native engine",
            summary.native_engine_supported_match,
            summary.native_engine_supported_mismatch,
            summary.native_engine_supported_accuracy,
            summary.native_engine_coverage,
        ),
        provenance_row(
            "Rule-id hand-port",
            summary.rule_id_hand_port_supported_match,
            summary.rule_id_hand_port_supported_mismatch,
            summary.rule_id_hand_port_supported_accuracy,
            summary.rule_id_hand_port_coverage,
        ),
        provenance_row(
            "Unknown",
            summary.unknown_provenance_supported_match,
            summary.unknown_provenance_supported_mismatch,
            summary.unknown_provenance_supported_accuracy,
            summary.unknown_provenance_coverage,
        ),
        String::new(),
        "Aggregate coverage includes both native engine and rule-id hand-port supported cases. Use native engine coverage to understand generic engine support."
            .to_owned(),
        String::new(),
        "## Gate".to_owned(),
        String::new(),
        "| Gate | Value |".to_owned(),
        "|---|---:|".to_owned(),
        format!(
            "| Minimum coverage | {} |",
            scoreboard
                .gate
                .min_coverage
                .map(percent)
                .unwrap_or_else(|| "not set".to_owned())
        ),
        format!(
            "| Maximum skipped unsupported | {} |",
            scoreboard
                .gate
                .max_skipped_unsupported
                .map(|value| value.to_string())
                .unwrap_or_else(|| "not set".to_owned())
        ),
        format!(
            "| Coverage threshold failed | {} |",
            scoreboard.gate.coverage_threshold_failed
        ),
        format!(
            "| Skipped unsupported threshold failed | {} |",
            scoreboard.gate.skipped_unsupported_threshold_failed
        ),
        format!("| Gate failed | {} |", scoreboard.gate.should_fail),
        String::new(),
        "## Upstream".to_owned(),
        String::new(),
        format!("- Repo: `{}`", scoreboard.upstream.repo),
        format!(
            "- Expected SHA: `{}`",
            scoreboard
                .upstream
                .expected_sha
                .as_deref()
                .unwrap_or("not recorded")
        ),
        format!(
            "- Observed SHA: `{}`",
            scoreboard
                .upstream
                .observed_sha
                .as_deref()
                .unwrap_or("not available")
        ),
        String::new(),
    ];

    if summary.unverified_synthetic_oracle_match > 0 {
        lines.push("## Synthetic Oracle Notice".to_owned());
        lines.push(String::new());
        lines.push(format!(
            "- {} case(s) use unverified synthetic oracle classification because official `results.csv` is absent.",
            summary.unverified_synthetic_oracle_match
        ));
        lines.push(
            "- These cases keep the score bucket out of `no_official_oracle`, but they are not evidence of an official oracle match."
                .to_owned(),
        );
        lines.push(
            "- CI should treat them as reportable warnings, not correctness failures.".to_owned(),
        );
        lines.push(String::new());
    }

    if !scoreboard.upstream.warnings.is_empty() {
        lines.push("## Warnings".to_owned());
        lines.push(String::new());
        for warning in &scoreboard.upstream.warnings {
            lines.push(format!("- {warning}"));
        }
        lines.push(String::new());
    }

    push_case_section(
        &mut lines,
        "Supported Mismatches",
        scoreboard,
        ScoreBucket::SupportedMismatch,
        50,
    );
    push_case_section(
        &mut lines,
        "Harness Errors",
        scoreboard,
        ScoreBucket::HarnessError,
        50,
    );
    push_case_section(
        &mut lines,
        "Mixed Skipped And Issues",
        scoreboard,
        ScoreBucket::MixedSkippedAndIssues,
        50,
    );
    push_case_section(
        &mut lines,
        "No Official Oracle Sample",
        scoreboard,
        ScoreBucket::NoOfficialOracle,
        10,
    );
    push_synthetic_reason_section(&mut lines, scoreboard);
    push_skipped_reason_section(&mut lines, scoreboard);
    push_case_section(
        &mut lines,
        "Skipped Unsupported Sample",
        scoreboard,
        ScoreBucket::SkippedUnsupported,
        10,
    );

    lines.join("\n") + "\n"
}

fn push_synthetic_reason_section(lines: &mut Vec<String>, scoreboard: &Scoreboard) {
    let mut counts = BTreeMap::<String, usize>::new();
    for case in scoreboard
        .cases
        .iter()
        .filter(|case| case.bucket == ScoreBucket::SupportedMatch)
    {
        let Some(reason) = &case.reason else {
            continue;
        };
        if reason.contains("synthetic") {
            *counts.entry(reason.clone()).or_default() += 1;
        }
    }

    if counts.is_empty() {
        return;
    }

    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    lines.push("## Synthetic Oracle Reasons".to_owned());
    lines.push(String::new());
    for (reason, count) in counts {
        lines.push(format!("- `{reason}`: {count} case(s)"));
    }
    lines.push(String::new());
}

fn push_skipped_reason_section(lines: &mut Vec<String>, scoreboard: &Scoreboard) {
    let mut counts = BTreeMap::<String, usize>::new();
    for case in scoreboard
        .cases
        .iter()
        .filter(|case| case.bucket == ScoreBucket::SkippedUnsupported)
    {
        for reason in &case.skipped_reasons {
            *counts.entry(reason.clone()).or_default() += 1;
        }
    }

    if counts.is_empty() {
        return;
    }

    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    lines.push("## Skipped Unsupported Reasons".to_owned());
    lines.push(String::new());
    for (reason, count) in counts {
        lines.push(format!("- `{reason}`: {count} case(s)"));
    }
    lines.push(String::new());
}

fn push_case_section(
    lines: &mut Vec<String>,
    title: &str,
    scoreboard: &Scoreboard,
    bucket: ScoreBucket,
    limit: usize,
) {
    let cases = scoreboard
        .cases
        .iter()
        .filter(|case| case.bucket == bucket)
        .take(limit)
        .collect::<Vec<_>>();
    if cases.is_empty() {
        return;
    }

    lines.push(format!("## {title}"));
    lines.push(String::new());
    for case in cases {
        let reason = case
            .reason
            .as_deref()
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default();
        lines.push(format!(
            "- `{}` {}/{}{}{}{} official={} candidate={}",
            case.rule_id,
            case.case_kind,
            case.case_id,
            provenance_text(&case.execution_provenance),
            reason,
            provenance_suffix(&case.execution_provenance),
            count_text(case.official_issue_count),
            count_text(case.candidate_issue_count)
        ));
    }
    lines.push(String::new());
}

fn provenance_row(
    label: &str,
    matches: usize,
    mismatches: usize,
    accuracy: Option<f64>,
    coverage: Option<f64>,
) -> String {
    format!(
        "| {label} | {matches} | {mismatches} | {} | {} |",
        percent_or_na(accuracy),
        percent_or_na(coverage)
    )
}

fn provenance_text(provenance: &ExecutionProvenance) -> &'static str {
    provenance.as_str()
}

fn count_text(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_owned())
}

fn provenance_suffix(provenance: &ExecutionProvenance) -> &'static str {
    match provenance {
        ExecutionProvenance::NativeEngine => " provenance=native_engine",
        ExecutionProvenance::RuleIdHandPort => " provenance=rule_id_hand_port",
        ExecutionProvenance::Unknown => "",
    }
}

fn percent_or_na(value: Option<f64>) -> String {
    value.map(percent).unwrap_or_else(|| "n/a".to_owned())
}

fn percent(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::open_rules::score::{
        ExecutionProvenance, ScoreBucket, ScoreSummary, Scoreboard, ScoredCase,
    };
    use crate::open_rules::upstream::UpstreamInfo;

    use super::*;

    #[test]
    fn writes_json_and_markdown_scoreboard() {
        let dir = tempdir().expect("tempdir");
        let scoreboard = Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("observed".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: vec!["warning text".to_owned()],
            },
            vec![
                ScoredCase {
                    scope: "Published".to_owned(),
                    rule_id: "CORE-000005".to_owned(),
                    case_kind: "negative".to_owned(),
                    case_id: "01".to_owned(),
                    case_dir: "case".into(),
                    official_results_csv: "official.csv".into(),
                    candidate_report_csv: "report.csv".into(),
                    execution_provenance: ExecutionProvenance::NativeEngine,
                    bucket: ScoreBucket::SupportedMismatch,
                    reason: None,
                    skipped_reasons: Vec::new(),
                    official_issue_count: Some(1),
                    candidate_issue_count: Some(1),
                    missing: Vec::new(),
                    extra: Vec::new(),
                },
                ScoredCase {
                    scope: "Published".to_owned(),
                    rule_id: "CORE-000006".to_owned(),
                    case_kind: "positive".to_owned(),
                    case_id: "01".to_owned(),
                    case_dir: "case".into(),
                    official_results_csv: "official.csv".into(),
                    candidate_report_csv: "report.csv".into(),
                    execution_provenance: ExecutionProvenance::Unknown,
                    bucket: ScoreBucket::SkippedUnsupported,
                    reason: Some("candidate skipped rows: unsupported_operator".to_owned()),
                    skipped_reasons: vec!["unsupported_operator".to_owned()],
                    official_issue_count: Some(0),
                    candidate_issue_count: Some(0),
                    missing: Vec::new(),
                    extra: Vec::new(),
                },
                ScoredCase {
                    scope: "Published".to_owned(),
                    rule_id: "CORE-000007".to_owned(),
                    case_kind: "positive".to_owned(),
                    case_id: "01".to_owned(),
                    case_dir: "case".into(),
                    official_results_csv: "missing-results.csv".into(),
                    candidate_report_csv: "report.csv".into(),
                    execution_provenance: ExecutionProvenance::RuleIdHandPort,
                    bucket: ScoreBucket::SupportedMatch,
                    reason: Some(
                        "missing official results.csv; unverified synthetic candidate oracle"
                            .to_owned(),
                    ),
                    skipped_reasons: Vec::new(),
                    official_issue_count: Some(0),
                    candidate_issue_count: Some(0),
                    missing: Vec::new(),
                    extra: Vec::new(),
                },
            ],
        );

        write_scoreboard(dir.path(), &scoreboard).expect("write scoreboard");

        let json = fs::read_to_string(dir.path().join("scoreboard.json")).expect("read json");
        let markdown = fs::read_to_string(dir.path().join("summary.md")).expect("read markdown");

        assert!(json.contains("\"supported_mismatch\": 1"));
        assert!(markdown.contains("# CDISC Open Rules Oracle Compatibility"));
        assert!(markdown.contains("CORE-000005"));
        assert!(markdown.contains("| Official oracle match | 0 |"));
        assert!(markdown.contains("| Synthetic oracle match | 1 |"));
        assert!(markdown.contains("| Unverified synthetic oracle match | 1 |"));
        assert!(markdown.contains("| Mixed skipped and issues | 0 |"));
        assert!(markdown.contains("## Execution Provenance"));
        assert!(markdown.contains("| Native engine | 0 | 1 | 0.00% | 33.33% |"));
        assert!(markdown.contains("| Rule-id hand-port | 1 | 0 | 100.00% | 33.33% |"));
        assert!(markdown.contains(
            "Aggregate coverage includes both native engine and rule-id hand-port supported cases."
        ));
        assert!(markdown.contains("provenance=native_engine"));
        assert!(markdown.contains("## Synthetic Oracle Notice"));
        assert!(markdown.contains("## Synthetic Oracle Reasons"));
        assert!(markdown.contains("## Skipped Unsupported Reasons"));
        assert!(markdown.contains("- `unsupported_operator`: 1 case(s)"));
        assert!(markdown.contains("warning text"));
        assert!(scoreboard.summary.should_fail());
        assert_eq!(
            scoreboard.summary,
            ScoreSummary::from_cases(&scoreboard.cases)
        );
    }
}
