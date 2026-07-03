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
    pub log: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportDocument {
    pub metadata: ReportMetadata,
    pub summary: ReportSummary,
    pub results: Vec<RuleValidationResult>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutputFormat {
    #[default]
    Both,
    Json,
    Csv,
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
    pub engine_version: Option<String>,
    pub standard: Option<String>,
    pub standard_version: Option<String>,
    pub log_level: Option<String>,
    pub rule_count: Option<usize>,
    pub dataset_count: Option<usize>,
    pub define_xml_count: Option<usize>,
    pub ct_count: Option<usize>,
    pub external_dictionary_count: Option<usize>,
}

impl Default for ReportMetadata {
    fn default() -> Self {
        Self {
            schema_version: "1.0".to_owned(),
            engine: "core-rs".to_owned(),
            engine_version: None,
            standard: None,
            standard_version: None,
            log_level: None,
            rule_count: None,
            dataset_count: None,
            define_xml_count: None,
            ct_count: None,
            external_dictionary_count: None,
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
    let log = if should_write_log(&options.metadata) {
        Some(write_log_report(
            output_dir.join("validation.log"),
            results,
            &options.metadata,
        )?)
    } else {
        None
    };

    Ok(WrittenReports { json, csv, log })
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

pub fn write_log_report(
    path: impl AsRef<Path>,
    results: &[RuleValidationResult],
    metadata: &ReportMetadata,
) -> Result<PathBuf> {
    let path = path.as_ref();
    create_parent_dir(path)?;
    let summary = ReportSummary::from_results(results);
    let mut file = File::create(path).map_err(|source| ReportError::CreateFile {
        path: path.to_path_buf(),
        source,
    })?;

    writeln!(
        file,
        "schema_version={}",
        escape_log_field(&metadata.schema_version)
    )
    .map_err(|source| ReportError::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    writeln!(file, "engine={}", escape_log_field(&metadata.engine)).map_err(|source| {
        ReportError::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if let Some(version) = &metadata.engine_version {
        writeln!(file, "engine_version={}", escape_log_field(version)).map_err(|source| {
            ReportError::WriteFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    if let Some(level) = &metadata.log_level {
        writeln!(file, "log_level={}", escape_log_field(level)).map_err(|source| {
            ReportError::WriteFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    if let Some(standard) = &metadata.standard {
        writeln!(file, "standard={}", escape_log_field(standard)).map_err(|source| {
            ReportError::WriteFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    if let Some(version) = &metadata.standard_version {
        writeln!(file, "standard_version={}", escape_log_field(version)).map_err(|source| {
            ReportError::WriteFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    for (name, value) in [
        ("rule_count", metadata.rule_count),
        ("dataset_count", metadata.dataset_count),
        ("define_xml_count", metadata.define_xml_count),
        ("ct_count", metadata.ct_count),
        (
            "external_dictionary_count",
            metadata.external_dictionary_count,
        ),
    ] {
        if let Some(value) = value {
            writeln!(file, "{name}={value}").map_err(|source| ReportError::WriteFile {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }
    writeln!(
        file,
        "summary total_results={} passed={} failed={} skipped={} error_count={}",
        summary.total_results, summary.passed, summary.failed, summary.skipped, summary.error_count
    )
    .map_err(|source| ReportError::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    for result in results {
        writeln!(
            file,
            "result rule_id={} status={} dataset={} domain={} errors={} skipped_reason={}",
            escape_log_field(&result.rule_id),
            execution_status_name(&result.execution_status),
            escape_log_field(&result.dataset),
            escape_log_field(&result.domain.clone().unwrap_or_default()),
            result.error_count,
            skipped_reason_name(result),
        )
        .map_err(|source| ReportError::WriteFile {
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

fn should_write_log(metadata: &ReportMetadata) -> bool {
    metadata
        .log_level
        .as_deref()
        .is_some_and(|level| !level.eq_ignore_ascii_case("disabled"))
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
    let field = if field
        .trim_start_matches(|character: char| character.is_whitespace())
        .starts_with(['=', '+', '-', '@'])
    {
        format!("'{field}")
    } else {
        field.to_owned()
    };
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field
    }
}

fn escape_log_field(field: &str) -> String {
    field
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            _ if character.is_control() => {
                format!("\\u{{{:x}}}", character as u32).chars().collect()
            }
            _ => vec![character],
        })
        .collect()
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
                execution_provenance: None,
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
                execution_provenance: None,
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
    fn write_csv_report_escapes_spreadsheet_formula_prefixes() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.csv");
        let results = vec![RuleValidationResult {
            rule_id: "=CORE-TEST-0001".to_owned(),
            execution_status: ExecutionStatus::Failed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: "+AE".to_owned(),
            domain: Some("-AE".to_owned()),
            message: "@message".to_owned(),
            error_count: 1,
            errors: vec![ValidationIssue {
                rule_id: "=CORE-TEST-0001".to_owned(),
                dataset: "+AE".to_owned(),
                domain: Some("-AE".to_owned()),
                row: Some(1),
                variables: vec!["@DOMAIN".to_owned()],
                message: "@message".to_owned(),
                usubjid: Some(" \t=SUBJ".to_owned()),
                seq: Some("+1".to_owned()),
            }],
        }];

        write_csv_report(&path, &results).expect("write csv");
        let source = fs::read_to_string(path).expect("read csv");

        assert!(source
            .contains("'=CORE-TEST-0001,failed,'+AE,'-AE,1,'@DOMAIN,'@message,1,,' \t=SUBJ,'+1"));
    }

    #[test]
    fn write_log_report_escapes_control_characters() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("validation.log");
        let results = vec![RuleValidationResult {
            rule_id: "CORE\nINJECT".to_owned(),
            execution_status: ExecutionStatus::Passed,
            execution_provenance: None,
            skipped_reason: None,
            dataset: "AE\tTAB".to_owned(),
            domain: Some("DM\rCR".to_owned()),
            message: String::new(),
            error_count: 0,
            errors: Vec::new(),
        }];

        write_log_report(
            &path,
            &results,
            &ReportMetadata {
                engine_version: Some("0.1\nbad=true".to_owned()),
                ..Default::default()
            },
        )
        .expect("write log");
        let source = fs::read_to_string(path).expect("read log");

        assert!(source.contains("engine_version=0.1\\nbad=true"));
        assert!(source.contains("rule_id=CORE\\nINJECT"));
        assert!(source.contains("dataset=AE\\tTAB"));
        assert!(source.contains("domain=DM\\rCR"));
        assert!(!source.lines().any(|line| line == "bad=true"));
    }

    #[test]
    fn write_reports_writes_default_report_names() {
        let dir = tempdir().expect("tempdir");

        let written = write_reports(dir.path(), &sample_results()).expect("write reports");

        assert_eq!(written.json, Some(dir.path().join("report.json")));
        assert_eq!(written.csv, Some(dir.path().join("report.csv")));
        assert_eq!(written.log, None);
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
                    engine_version: Some("0.1.0".to_owned()),
                    standard: Some("SDTMIG".to_owned()),
                    standard_version: Some("3.4".to_owned()),
                    log_level: Some("info".to_owned()),
                    rule_count: Some(2),
                    dataset_count: Some(1),
                    ..Default::default()
                },
            },
        )
        .expect("write json report");

        assert!(written.json.expect("json").exists());
        assert_eq!(written.csv, None);
        assert!(written.log.expect("log").exists());
        assert!(!dir.path().join("report.csv").exists());

        let document: ReportDocument =
            serde_json::from_str(&fs::read_to_string(dir.path().join("report.json")).unwrap())
                .expect("document");
        assert_eq!(document.metadata.schema_version, "1.0");
        assert_eq!(document.metadata.engine, "core-rs");
        assert_eq!(document.metadata.engine_version.as_deref(), Some("0.1.0"));
        assert_eq!(document.metadata.standard.as_deref(), Some("SDTMIG"));
        assert_eq!(document.metadata.standard_version.as_deref(), Some("3.4"));
        assert_eq!(document.metadata.log_level.as_deref(), Some("info"));
        assert_eq!(document.metadata.rule_count, Some(2));
        assert_eq!(document.metadata.dataset_count, Some(1));

        let log = fs::read_to_string(dir.path().join("validation.log")).expect("read log");
        assert!(log.contains("schema_version=1.0"));
        assert!(log.contains("engine=core-rs"));
        assert!(log.contains("summary total_results=2 passed=1 failed=1 skipped=0 error_count=2"));
        assert!(log.contains("result rule_id=CORE-TEST-0001 status=failed dataset=AE"));
    }
}
