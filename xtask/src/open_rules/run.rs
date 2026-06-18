//! Open Rules execution harness.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use core_api::{run_validation, DatasetLoader, ValidateRequest};
use core_report::ReportOutputFormat;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::score::relative_candidate_report_path;

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,
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
                message: source.to_string(),
            }),
        }
    }

    Ok(RunSummary {
        total_cases: cases.len(),
        succeeded,
        failed: errors.len(),
        errors,
    })
}

fn run_case(case: &OpenRulesCase, core_rs_results_root: &Path) -> Result<()> {
    let report_path = core_rs_results_root.join(relative_candidate_report_path(case));
    let output_dir = report_path
        .parent()
        .context("candidate report path has no parent")?;
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![case.rule_path.clone()],
        dataset_paths: vec![case.data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        include_rules: vec![case.rule_id.clone()],
        exclude_rules: Vec::new(),
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

    if outcome
        .reports
        .and_then(|reports| reports.csv)
        .is_some_and(|path| path.is_file())
    {
        Ok(())
    } else {
        Err(anyhow::anyhow!("candidate report.csv was not written"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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
        assert!(candidate_root
            .path()
            .join("Published/CORE-OPEN-0001/positive/01/report.csv")
            .is_file());
        assert!(candidate_root
            .path()
            .join("Published/CORE-OPEN-0001/negative/01/report.csv")
            .is_file());
    }
}
