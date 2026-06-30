//! Open Rules execution harness.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use core_api::{run_validation, DatasetLoader, ValidateRequest};
use core_report::ReportOutputFormat;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::score::{
    execution_provenance_for_rule_id, relative_candidate_report_path, ExecutionProvenance,
    ScoreArgs,
};
use crate::open_rules::upstream::{ensure_strict_lock_matches, load_upstream_info};

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct RunScoreArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,

    #[arg(long)]
    pub strict_lock: bool,

    #[arg(
        long,
        value_name = "RATIO",
        value_parser = crate::open_rules::score::parse_coverage_ratio
    )]
    pub min_coverage: Option<f64>,

    #[arg(long, value_name = "COUNT")]
    pub max_skipped_unsupported: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSummary {
    pub total_cases: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub errors: Vec<RunCaseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunCaseError {
    pub scope: String,
    pub rule_id: String,
    pub case_kind: String,
    pub case_id: String,
    pub message: String,
}

pub fn run(args: RunArgs) -> Result<bool> {
    let cases = discover_cases(&args.open_rules_root, &args.scope)?;
    let summary = run_cases(&cases, &args.core_rs_results_root)?;
    println!(
        "open-rules run completed: {} succeeded, {} failed, {} total",
        summary.succeeded, summary.failed, summary.total_cases
    );
    Ok(summary.failed > 0)
}

pub fn run_score(args: RunScoreArgs) -> Result<bool> {
    if args.strict_lock {
        let upstream = load_upstream_info(&args.open_rules_root)?;
        ensure_strict_lock_matches(&upstream)?;
    }

    let cases = discover_cases(&args.open_rules_root, &args.scope)?;
    let run_summary = run_cases(&cases, &args.core_rs_results_root)?;
    let score_failed = crate::open_rules::score::run(ScoreArgs {
        open_rules_root: args.open_rules_root,
        core_rs_results_root: args.core_rs_results_root,
        out: args.out,
        scope: args.scope,
        min_coverage: args.min_coverage,
        max_skipped_unsupported: args.max_skipped_unsupported,
    })?;

    Ok(run_summary.failed > 0 || score_failed)
}

pub fn run_cases(cases: &[OpenRulesCase], core_rs_results_root: &Path) -> Result<RunSummary> {
    let mut errors = Vec::new();
    let mut succeeded = 0;

    for case in cases {
        match run_case(case, core_rs_results_root) {
            Ok(()) => succeeded += 1,
            Err(source) => errors.push(RunCaseError {
                scope: case.scope.clone(),
                rule_id: case.rule_id.clone(),
                case_kind: case.case_kind.as_str().to_owned(),
                case_id: case.case_id.clone(),
                message: format!("{source:#}"),
            }),
        }
    }

    let summary = RunSummary {
        total_cases: cases.len(),
        succeeded,
        failed: errors.len(),
        errors,
    };
    write_run_summary(core_rs_results_root, &summary)?;

    Ok(summary)
}

fn write_run_summary(core_rs_results_root: &Path, summary: &RunSummary) -> Result<()> {
    fs::create_dir_all(core_rs_results_root)
        .with_context(|| format!("create {}", core_rs_results_root.display()))?;
    let path = core_rs_results_root.join("run-summary.json");
    let file = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    serde_json::to_writer_pretty(file, summary).with_context(|| format!("write {}", path.display()))
}

fn run_case(case: &OpenRulesCase, core_rs_results_root: &Path) -> Result<()> {
    let report_path = core_rs_results_root.join(relative_candidate_report_path(case));
    let output_dir = report_path
        .parent()
        .context("candidate report path has no parent")?;
    let (standard, standard_version) = standard_filter_from_env(&case.env);
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![case.rule_path.clone()],
        dataset_paths: vec![case.data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        include_rules: vec![case.rule_id.clone()],
        exclude_rules: Vec::new(),
        standard: standard.clone(),
        standard_version,
        output_format: ReportOutputFormat::Csv,
        output_dir: Some(output_dir.to_path_buf()),
        ..Default::default()
    })
    .with_context(|| {
        format!(
            "run {} {}/{}",
            case.rule_id,
            case.case_kind.as_str(),
            case.case_id
        )
    })?;

    let Some(report_csv) = outcome.reports.and_then(|reports| reports.csv) else {
        return Err(anyhow::anyhow!("candidate report.csv was not written"));
    };
    if !report_csv.is_file() {
        return Err(anyhow::anyhow!("candidate report.csv was not written"));
    }

    annotate_candidate_report_provenance(
        &report_csv,
        execution_provenance_for_rule_id(&case.rule_id),
    )
}

fn annotate_candidate_report_provenance(
    report_csv: &Path,
    provenance: ExecutionProvenance,
) -> Result<()> {
    let source =
        fs::read_to_string(report_csv).with_context(|| format!("read {}", report_csv.display()))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(source.as_bytes());
    let headers = reader
        .headers()
        .with_context(|| format!("read CSV headers {}", report_csv.display()))?
        .clone();
    if headers
        .iter()
        .any(|header| header.trim().eq_ignore_ascii_case("execution_provenance"))
    {
        return Ok(());
    }

    let mut output = Vec::new();
    {
        let mut writer = csv::Writer::from_writer(&mut output);
        let mut annotated_headers = headers.clone();
        annotated_headers.push_field("execution_provenance");
        writer
            .write_record(&annotated_headers)
            .with_context(|| format!("write CSV headers {}", report_csv.display()))?;
        for record in reader.records() {
            let mut record =
                record.with_context(|| format!("read CSV record {}", report_csv.display()))?;
            record.push_field(provenance.as_str());
            writer
                .write_record(&record)
                .with_context(|| format!("write CSV record {}", report_csv.display()))?;
        }
        writer
            .flush()
            .with_context(|| format!("flush CSV {}", report_csv.display()))?;
    }
    fs::write(report_csv, output).with_context(|| format!("write {}", report_csv.display()))
}

fn standard_filter_from_env(env: &BTreeMap<String, String>) -> (Option<String>, Option<String>) {
    let standard = env_value(env, "PRODUCT");
    let version = env_value(env, "VERSION").map(|value| value.replace('-', "."));
    (standard, version)
}

fn env_value(env: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env.iter()
        .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
        .map(|(_key, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser as _;
    use tempfile::tempdir;

    use crate::open_rules::discovery::discover_cases;

    use super::*;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn run_cases_writes_mirrored_candidate_reports() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_executable");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");
        let candidate_root = tempdir().expect("candidate root");

        let summary = run_cases(&cases, candidate_root.path()).expect("run cases");

        assert_eq!(summary.total_cases, 2);
        assert_eq!(summary.succeeded, 2);
        assert!(summary.errors.is_empty());
        let positive_report = candidate_root
            .path()
            .join("Published/CORE-OPEN-0001/positive/01/report.csv");
        let negative_report = candidate_root
            .path()
            .join("Published/CORE-OPEN-0001/negative/01/report.csv");
        assert!(positive_report.is_file());
        assert!(negative_report.is_file());
        let report_csv = std::fs::read_to_string(&negative_report).expect("read report csv");
        assert!(report_csv
            .lines()
            .next()
            .is_some_and(|header| header.contains("execution_provenance")));
        assert!(report_csv.contains("native_engine"));
        let run_summary = std::fs::read_to_string(candidate_root.path().join("run-summary.json"))
            .expect("read run summary");
        assert!(run_summary.contains("\"total_cases\": 2"));
        assert!(run_summary.contains("\"failed\": 0"));
    }

    #[test]
    fn run_cases_error_messages_include_source_chain() {
        let dir = tempdir().expect("tempdir");
        let case_dir = dir.path().join("Published/CORE-MISSING/negative/01");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-MISSING".to_owned(),
            rule_dir: dir.path().join("Published/CORE-MISSING"),
            rule_path: dir.path().join("Published/CORE-MISSING/rule.yml"),
            case_kind: crate::open_rules::discovery::CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: case_dir.clone(),
            data_dir: case_dir.join("data"),
            env_path: case_dir.join("data/.env"),
            env: Default::default(),
            datasets_path: case_dir.join("data/_datasets.csv"),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: case_dir.join("data/_variables.csv"),
            variables: Vec::new(),
            official_results_csv: case_dir.join("results/results.csv"),
            has_official_results: false,
        };
        let candidate_root = tempdir().expect("candidate root");

        let summary = run_cases(&[case], candidate_root.path()).expect("run cases");

        assert_eq!(summary.failed, 1);
        assert!(summary.errors[0]
            .message
            .contains("run CORE-MISSING negative/01"));
        assert!(summary.errors[0].message.contains("failed to load rules"));
    }

    #[test]
    fn standard_filter_from_env_uses_product_and_normalized_version() {
        let env = BTreeMap::from([
            ("PRODUCT".to_owned(), "SENDIG".to_owned()),
            ("VERSION".to_owned(), "3-1".to_owned()),
        ]);

        let (standard, version) = standard_filter_from_env(&env);

        assert_eq!(standard.as_deref(), Some("SENDIG"));
        assert_eq!(version.as_deref(), Some("3.1"));
    }

    #[test]
    fn run_score_cli_rejects_invalid_min_coverage() {
        let valid = RunScoreArgs::try_parse_from([
            "run-score",
            "--open-rules-root",
            "open",
            "--core-rs-results-root",
            "candidate",
            "--out",
            "scoreboard",
            "--min-coverage",
            "1.0",
        ])
        .expect("valid min coverage");
        assert_eq!(valid.min_coverage, Some(1.0));

        for invalid in ["-0.1", "1.1", "NaN", "inf", "Infinity"] {
            let result = RunScoreArgs::try_parse_from([
                "run-score",
                "--open-rules-root",
                "open",
                "--core-rs-results-root",
                "candidate",
                "--out",
                "scoreboard",
                "--min-coverage",
                invalid,
            ]);
            assert!(result.is_err(), "{invalid} should be rejected");
        }
    }

    #[test]
    fn run_score_generates_reports_and_scoreboard() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_executable");
        let candidate_root = tempdir().expect("candidate root");
        let scoreboard_root = tempdir().expect("scoreboard root");

        let should_fail = run_score(RunScoreArgs {
            open_rules_root,
            core_rs_results_root: candidate_root.path().to_path_buf(),
            out: scoreboard_root.path().to_path_buf(),
            scope: Vec::new(),
            strict_lock: false,
            min_coverage: None,
            max_skipped_unsupported: None,
        })
        .expect("run score");

        assert!(!should_fail);
        let scoreboard = std::fs::read_to_string(scoreboard_root.path().join("scoreboard.json"))
            .expect("read scoreboard");
        assert!(scoreboard.contains("\"total_cases\": 2"));
        assert!(scoreboard.contains("\"supported_match\": 2"));
        assert!(scoreboard.contains("\"native_engine_supported_match\": 2"));
        assert!(scoreboard.contains("\"unknown_provenance_supported_match\": 0"));
    }
}
