//! Normalize official CORE and core-rust CSV reports to structural issue keys.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSource {
    Official,
    CoreRs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct IssueKey {
    pub rule_id: String,
    pub dataset: String,
    pub domain: String,
    pub row: String,
    pub variables: Vec<String>,
    pub usubjid: String,
    pub seq: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedCsv {
    pub path: PathBuf,
    pub row_count: usize,
    pub skipped_row_count: usize,
    pub issue_count: usize,
    pub issues: Vec<IssueKey>,
}

pub fn normalize_csv(
    path: &Path,
    source: ReportSource,
    default_rule_id: Option<&str>,
) -> Result<NormalizedCsv> {
    let rows = read_rows(path)?;
    let skipped_row_count = match source {
        ReportSource::Official => 0,
        ReportSource::CoreRs => rows.iter().filter(|row| row_is_core_rs_skipped(row)).count(),
    };
    let issue_rows = rows
        .iter()
        .filter(|row| match source {
            ReportSource::Official => true,
            ReportSource::CoreRs => !row_is_core_rs_non_issue(row),
        })
        .collect::<Vec<_>>();
    let issues = issue_rows
        .into_iter()
        .map(|row| normalize_row(row, default_rule_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Ok(NormalizedCsv {
        path: path.to_path_buf(),
        row_count: rows.len(),
        skipped_row_count,
        issue_count: issues.len(),
        issues,
    })
}

pub fn normalize_scalar(value: &str) -> String {
    let text = value.trim();
    if matches!(
        text.to_ascii_lowercase().as_str(),
        "" | "null" | "none" | "nan" | "na" | "n/a"
    ) {
        String::new()
    } else {
        text.to_owned()
    }
}

fn read_rows(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("open CSV {}", path.display()))?;
    let headers = reader
        .headers()
        .with_context(|| format!("read CSV headers {}", path.display()))?
        .iter()
        .map(|header| header.trim().to_owned())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.with_context(|| format!("read CSV record {}", path.display()))?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.clone(), value.to_owned()))
            .collect::<BTreeMap<_, _>>();
        rows.push(row);
    }
    Ok(rows)
}

fn normalize_row(row: &BTreeMap<String, String>, default_rule_id: Option<&str>) -> IssueKey {
    let rule_id = first(row, &["rule_id", "rule", "core_id", "core-id", "id"])
        .or_else(|| default_rule_id.map(str::to_owned))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let dataset = normalize_dataset_like(
        &first(
            row,
            &["dataset", "dataset_name", "Dataset", "domain", "domain_name"],
        )
        .unwrap_or_default(),
    );
    let domain = normalize_dataset_like(
        &first(row, &["domain", "domain_name"]).unwrap_or_else(|| dataset.clone()),
    );
    let row_number = first(
        row,
        &[
            "row",
            "row_number",
            "record",
            "Record",
            "record_number",
            "line",
            "line_number",
        ],
    )
    .unwrap_or_default();
    let variables = split_variables(
        &first(
            row,
            &[
                "variables",
                "variable",
                "Variable",
                "variable_name",
                "column",
                "columns",
            ],
        )
        .unwrap_or_default(),
    );
    let usubjid =
        first(row, &["usubjid", "USUBJID", "subject", "subject_id"]).unwrap_or_default();
    let seq = first(row, &["seq", "SEQ", "sequence", "sequence_number"]).unwrap_or_default();

    IssueKey {
        rule_id,
        dataset,
        domain,
        row: normalize_scalar(&row_number),
        variables,
        usubjid: normalize_scalar(&usubjid),
        seq: normalize_scalar(&seq),
    }
}

fn first(row: &BTreeMap<String, String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        row.iter()
            .find(|(key, _value)| key.trim().eq_ignore_ascii_case(name))
            .map(|(_key, value)| normalize_scalar(value))
            .filter(|value| !value.is_empty())
    })
}

fn normalize_dataset_like(value: &str) -> String {
    let value = normalize_scalar(value);
    let value = value
        .strip_suffix(".csv")
        .or_else(|| value.strip_suffix(".CSV"))
        .unwrap_or(value.as_str());
    value.to_ascii_uppercase()
}

fn split_variables(value: &str) -> Vec<String> {
    let value = normalize_scalar(value);
    if value.is_empty() {
        return Vec::new();
    }
    value
        .split(['|', ';', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_uppercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn row_is_core_rs_skipped(row: &BTreeMap<String, String>) -> bool {
    let status = first(row, &["execution_status", "status"]).unwrap_or_default();
    let skipped_reason = first(row, &["skipped_reason", "skip_reason"]).unwrap_or_default();
    status.eq_ignore_ascii_case("skipped") || !skipped_reason.is_empty()
}

fn row_is_core_rs_non_issue(row: &BTreeMap<String, String>) -> bool {
    let status = first(row, &["execution_status", "status"]).unwrap_or_default();
    status.eq_ignore_ascii_case("passed") || status.eq_ignore_ascii_case("skipped")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn normalizes_issue_keys_without_messages_or_variable_order() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,USUBJID,SEQ,Message\ncm.csv,2,CMTRT|CMSEQ,STUDY01-002,2,official text\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,failed,CM,CM,2,CMSEQ|CMTRT,candidate text,1,,STUDY01-002,2\n",
        )
        .expect("write candidate");

        let official =
            normalize_csv(&official, ReportSource::Official, Some("CORE-000001"))
                .expect("official");
        let candidate =
            normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000001"))
                .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
        assert_eq!(official.issues[0].dataset, "CM");
        assert_eq!(official.issues[0].variables, vec!["CMSEQ", "CMTRT"]);
    }

    #[test]
    fn core_rs_passed_and_skipped_rows_are_not_issue_keys() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("report.csv");
        fs::write(
            &report,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,passed,CM,CM,,,passed,0,,,\nCORE-000002,skipped,,,,,unsupported,0,unsupported_operator,,\n",
        )
        .expect("write report");

        let normalized = normalize_csv(&report, ReportSource::CoreRs, None).expect("normalize");

        assert_eq!(normalized.row_count, 2);
        assert_eq!(normalized.skipped_row_count, 1);
        assert_eq!(normalized.issue_count, 0);
        assert!(normalized.issues.is_empty());
    }

    #[test]
    fn nullish_values_normalize_to_empty_but_zero_and_dot_remain() {
        assert_eq!(normalize_scalar(" null "), "");
        assert_eq!(normalize_scalar("N/A"), "");
        assert_eq!(normalize_scalar("0"), "0");
        assert_eq!(normalize_scalar("."), ".");
    }
}
