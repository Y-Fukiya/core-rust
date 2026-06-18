#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use core_engine::{ExecutionStatus, RuleValidationResult, ValidationIssue};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ReportError>;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("failed to create report directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create report file {path}: {source}")]
    CreateFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write report file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize JSON report {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrittenReports {
    pub json: Option<PathBuf>,
    pub csv: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportDocument {
    pub metadata: ReportMetadata,
    pub summary: ReportSummary,
    pub results: Vec<RuleValidationResult>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutputFormat {
    Both,
    Json,
    Csv,
}

impl Default for ReportOutputFormat {
    fn default() -> Self {
        Self::Both
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportOptions {
    pub output_format: ReportOutputFormat,
    pub metadata: ReportMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportMetadata {
    pub schema_version: String,
    pub engine: String,
    pub standard: Option<String>,
    pub standard_version: Option<String>,
    pub log_level: Option<String>,
}

impl Default for ReportMetadata {
    fn default() -> Self {
        Self {
            schema_version: "1.0".to_owned(),
            engine: "core-rs".to_owned(),
            standard: None,
            standard_version: None,
            log_level: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportSummary {
    pub total_results: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CsvReportRow {
    pub rule_id: String,
    pub execution_status: String,
    pub skipped_reason: String,
    pub dataset: String,
    pub domain: String,
    pub row: String,
    pub variables: String,
    pub message: String,
    pub error_count: usize,
    pub usubjid: String,
    pub seq: String,
}

pub fn write_reports(
    output_dir: impl AsRef<Path>,
    results: &[RuleValidationResult],
) -> Result<WrittenReports> {
    write_reports_with_options(output_dir, results, &ReportOptions::default())
}

pub fn write_reports_with_options(
    output_dir: impl AsRef<Path>,
    results: &[RuleValidationResult],
    options: &ReportOptions,
) -> Result<WrittenReports> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|source| ReportError::CreateDir {
        path: output_dir.to_path_buf(),
        source,
    })?;

    let json = if matches!(
        options.output_format,
        ReportOutputFormat::Both | ReportOutputFormat::Json
    ) {
        Some(write_json_report_with_metadata(
            output_dir.join("report.json"),
            results,
            options.metadata.clone(),
        )?)
    } else {
        None
    };
    let csv = if matches!(
        options.output_format,
        ReportOutputFormat::Both | ReportOutputFormat::Csv
    ) {
        Some(write_csv_report(output_dir.join("report.csv"), results)?)
    } else {
        None
    };

    Ok(WrittenReports { json, csv })
}

pub fn write_json_report(
    path: impl AsRef<Path>,
    results: &[RuleValidationResult],
) -> Result<PathBuf> {
    write_json_report_with_metadata(path, results, ReportMetadata::default())
}

pub fn write_json_report_with_metadata(
    path: impl AsRef<Path>,
    results: &[RuleValidationResult],
    metadata: ReportMetadata,
) -> Result<PathBuf> {
    let path = path.as_ref();
    create_parent_dir(path)?;

    let file = File::create(path).map_err(|source| ReportError::CreateFile {
        path: path.to_path_buf(),
        source,
    })?;
    let document = ReportDocument {
        metadata,
        summary: ReportSummary::from_results(results),
        results: results.to_vec(),
    };
    serde_json::to_writer_pretty(file, &document).map_err(|source| ReportError::Json {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(path.to_path_buf())
}

impl ReportSummary {
    pub fn from_results(results: &[RuleValidationResult]) -> Self {
        Self {
            total_results: results.len(),
            passed: results
                .iter()
                .filter(|result| result.execution_status == ExecutionStatus::Passed)
                .count(),
            failed: results
                .iter()
                .filter(|result| result.execution_status == ExecutionStatus::Failed)
                .count(),
            skipped: results
                .iter()
                .filter(|result| result.execution_status == ExecutionStatus::Skipped)
                .count(),
            error_count: results.iter().map(|result| result.error_count).sum(),
        }
    }
}

pub fn write_csv_report(
    path: impl AsRef<Path>,
    results: &[RuleValidationResult],
) -> Result<PathBuf> {
    let path = path.as_ref();
    create_parent_dir(path)?;

    let rows = flatten_csv_rows(results);
    let mut file = File::create(path).map_err(|source| ReportError::CreateFile {
        path: path.to_path_buf(),
        source,
    })?;

    write_csv_line(&mut file, CSV_HEADERS).map_err(|source| ReportError::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    for row in rows {
        write_csv_line(&mut file, &row.to_fields()).map_err(|source| ReportError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    }

    Ok(path.to_path_buf())
}

pub fn flatten_csv_rows(results: &[RuleValidationResult]) -> Vec<CsvReportRow> {
    results
        .iter()
        .flat_map(|result| {
            if result.errors.is_empty() {
                vec![CsvReportRow::from_result_without_issue(result)]
            } else {
                result
                    .errors
                    .iter()
                    .map(|issue| CsvReportRow::from_issue(result, issue))
                    .collect()
            }
        })
        .collect()
}

const CSV_HEADERS: &[&str] = &[
    "rule_id",
    "execution_status",
    "dataset",
    "domain",
    "row",
    "variables",
    "message",
    "error_count",
    "skipped_reason",
    "usubjid",
    "seq",
];

impl CsvReportRow {
    fn from_result_without_issue(result: &RuleValidationResult) -> Self {
        Self {
            rule_id: result.rule_id.clone(),
            execution_status: execution_status_name(&result.execution_status).to_owned(),
            skipped_reason: skipped_reason_name(result),
            dataset: result.dataset.clone(),
            domain: result.domain.clone().unwrap_or_default(),
            row: String::new(),
            variables: String::new(),
            message: result.message.clone(),
            error_count: result.error_count,
            usubjid: String::new(),
            seq: String::new(),
        }
    }

    fn from_issue(result: &RuleValidationResult, issue: &ValidationIssue) -> Self {
        Self {
            rule_id: issue.rule_id.clone(),
            execution_status: execution_status_name(&result.execution_status).to_owned(),
            skipped_reason: skipped_reason_name(result),
            dataset: issue.dataset.clone(),
            domain: issue.domain.clone().unwrap_or_default(),
            row: issue.row.map(|row| row.to_string()).unwrap_or_default(),
            variables: issue.variables.join("|"),
            message: issue.message.clone(),
            error_count: result.error_count,
            usubjid: issue.usubjid.clone().unwrap_or_default(),
            seq: issue.seq.clone().unwrap_or_default(),
        }
    }

    fn to_fields(&self) -> [String; 11] {
        [
            self.rule_id.clone(),
            self.execution_status.clone(),
            self.dataset.clone(),
            self.domain.clone(),
            self.row.clone(),
            self.variables.clone(),
            self.message.clone(),
            self.error_count.to_string(),
            self.skipped_reason.clone(),
            self.usubjid.clone(),
            self.seq.clone(),
        ]
    }
}

fn execution_status_name(status: &ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Passed => "passed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Skipped => "skipped",
    }
}

fn skipped_reason_name(result: &RuleValidationResult) -> String {
    result
        .skipped_reason
        .as_ref()
        .map(|reason| {
            serde_json::to_string(reason)
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned()
        })
        .unwrap_or_default()
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| ReportError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    Ok(())
}

fn write_csv_line(mut writer: impl Write, fields: &[impl AsRef<str>]) -> std::io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            writer.write_all(b",")?;
        }
        writer.write_all(escape_csv_field(field.as_ref()).as_bytes())?;
    }
    writer.write_all(b"\n")
}

fn escape_csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use core_engine::{ExecutionStatus, RuleValidationResult, ValidationIssue};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    fn sample_results() -> Vec<RuleValidationResult> {
        vec![
            RuleValidationResult {
                rule_id: "CORE-TEST-0001".to_owned(),
                execution_status: ExecutionStatus::Failed,
                skipped_reason: None,
                dataset: "AE".to_owned(),
                domain: Some("AE".to_owned()),
                message: "DOMAIN must be AE".to_owned(),
                error_count: 2,
                errors: vec![
                    ValidationIssue {
                        rule_id: "CORE-TEST-0001".to_owned(),
                        dataset: "AE".to_owned(),
                        domain: Some("AE".to_owned()),
                        row: Some(2),
                        variables: vec!["DOMAIN".to_owned()],
                        message: "DOMAIN must be AE".to_owned(),
                        usubjid: Some("SUBJ2".to_owned()),
                        seq: Some("2".to_owned()),
                    },
                    ValidationIssue {
                        rule_id: "CORE-TEST-0001".to_owned(),
                        dataset: "AE".to_owned(),
                        domain: Some("AE".to_owned()),
                        row: Some(3),
                        variables: vec!["DOMAIN".to_owned(), "AESEQ".to_owned()],
                        message: "DOMAIN, AESEQ need review".to_owned(),
                        usubjid: Some("SUBJ3".to_owned()),
                        seq: Some("3".to_owned()),
                    },
                ],
            },
            RuleValidationResult {
                rule_id: "CORE-TEST-0002".to_owned(),
                execution_status: ExecutionStatus::Passed,
                skipped_reason: None,
                dataset: "CM".to_owned(),
                domain: Some("CM".to_owned()),
                message: "CM passed".to_owned(),
                error_count: 0,
                errors: Vec::new(),
            },
        ]
    }

    #[test]
    fn write_json_report_writes_results_document() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("report.json");
        let results = sample_results();

        let written = write_json_report(&path, &results).expect("write json");
        let source = fs::read_to_string(&written).expect("read json");
        let document: ReportDocument = serde_json::from_str(&source).expect("parse json");

        assert_eq!(document.summary.total_results, 2);
        assert_eq!(document.summary.passed, 1);
        assert_eq!(document.summary.failed, 1);
        assert_eq!(document.summary.error_count, 2);
        assert_eq!(document.results, results);
    }

    #[test]
    fn write_csv_report_writes_issue_and_pass_rows() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.csv");

        write_csv_report(&path, &sample_results()).expect("write csv");
        let source = fs::read_to_string(path).expect("read csv");

        assert_eq!(
            source,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-TEST-0001,failed,AE,AE,2,DOMAIN,DOMAIN must be AE,2,,SUBJ2,2\n\
CORE-TEST-0001,failed,AE,AE,3,DOMAIN|AESEQ,\"DOMAIN, AESEQ need review\",2,,SUBJ3,3\n\
CORE-TEST-0002,passed,CM,CM,,,CM passed,0,,,\n"
        );
    }

    #[test]
    fn write_reports_writes_default_report_names() {
        let dir = tempdir().expect("tempdir");

        let written = write_reports(dir.path(), &sample_results()).expect("write reports");

        assert_eq!(written.json, Some(dir.path().join("report.json")));
        assert_eq!(written.csv, Some(dir.path().join("report.csv")));
        assert!(written.json.expect("json report").exists());
        assert!(written.csv.expect("csv report").exists());
    }

    #[test]
    fn flatten_csv_rows_preserves_passed_results() {
        let rows = flatten_csv_rows(&sample_results());

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].execution_status, "failed");
        assert_eq!(rows[0].row, "2");
        assert_eq!(rows[1].variables, "DOMAIN|AESEQ");
        assert_eq!(rows[2].execution_status, "passed");
        assert_eq!(rows[2].error_count, 0);
    }

    #[test]
    fn write_reports_honors_selected_output_format() {
        let dir = tempdir().expect("tempdir");

        let written = write_reports_with_options(
            dir.path(),
            &sample_results(),
            &ReportOptions {
                output_format: ReportOutputFormat::Json,
                metadata: ReportMetadata {
                    standard: Some("SDTMIG".to_owned()),
                    standard_version: Some("3.4".to_owned()),
                    log_level: Some("info".to_owned()),
                    ..Default::default()
                },
            },
        )
        .expect("write json report");

        assert!(written.json.expect("json").exists());
        assert_eq!(written.csv, None);
        assert!(!dir.path().join("report.csv").exists());

        let document: ReportDocument =
            serde_json::from_str(&fs::read_to_string(dir.path().join("report.json")).unwrap())
                .expect("document");
        assert_eq!(document.metadata.schema_version, "1.0");
        assert_eq!(document.metadata.engine, "core-rs");
        assert_eq!(document.metadata.standard.as_deref(), Some("SDTMIG"));
        assert_eq!(document.metadata.standard_version.as_deref(), Some("3.4"));
        assert_eq!(document.metadata.log_level.as_deref(), Some("info"));
    }
}
