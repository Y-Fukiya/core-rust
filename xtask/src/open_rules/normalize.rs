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
    pub skipped_reasons: BTreeMap<String, usize>,
    pub issue_count: usize,
    pub issues: Vec<IssueKey>,
}

pub fn normalize_csv(
    path: &Path,
    source: ReportSource,
    default_rule_id: Option<&str>,
) -> Result<NormalizedCsv> {
    let rows =
        normalize_official_dataset_name_operation_rows(read_rows(path)?, source, default_rule_id);
    let rows = normalize_official_core_000007_usubjid_rows(rows, source, default_rule_id);
    let skipped_row_count = match source {
        ReportSource::Official => 0,
        ReportSource::CoreRs => rows
            .iter()
            .filter(|row| row_is_core_rs_skipped(row))
            .count(),
    };
    let skipped_reasons = match source {
        ReportSource::Official => BTreeMap::new(),
        ReportSource::CoreRs => collect_core_rs_skipped_reasons(&rows),
    };
    let issue_rows = rows
        .iter()
        .filter(|row| match source {
            ReportSource::Official => !row_is_official_dataset_only_blank_marker(row),
            ReportSource::CoreRs => !row_is_core_rs_non_issue(row),
        })
        .collect::<Vec<_>>();
    let issues = issue_rows
        .into_iter()
        .flat_map(|row| expand_issue_key(normalize_row(row, default_rule_id)))
        .collect::<Vec<_>>();

    Ok(NormalizedCsv {
        path: path.to_path_buf(),
        row_count: rows.len(),
        skipped_row_count,
        skipped_reasons,
        issue_count: issues.len(),
        issues,
    })
}

fn normalize_official_dataset_name_operation_rows(
    mut rows: Vec<BTreeMap<String, String>>,
    source: ReportSource,
    default_rule_id: Option<&str>,
) -> Vec<BTreeMap<String, String>> {
    if source != ReportSource::Official {
        return rows;
    }
    let rule_id = default_rule_id.unwrap_or_default().to_ascii_uppercase();
    let row_value = match rule_id.as_str() {
        "CORE-000539" => Some("1"),
        "CORE-000540" => Some(""),
        _ => None,
    };
    let Some(row_value) = row_value else {
        return rows;
    };

    let dataset_by_record = rows
        .iter()
        .filter(|row| {
            first(row, &["Variable", "variable"])
                .is_some_and(|variable| variable.eq_ignore_ascii_case("dataset_name"))
        })
        .filter_map(|row| {
            let record = first(row, &["Record", "record"])?;
            let dataset = first(row, &["Value", "value"])?;
            Some((record, normalize_dataset_like(&dataset)))
        })
        .collect::<BTreeMap<_, _>>();
    if dataset_by_record.is_empty() {
        return rows;
    }

    for row in &mut rows {
        let Some(record) = first(row, &["Record", "record"]) else {
            continue;
        };
        let Some(dataset) = dataset_by_record.get(&record) else {
            continue;
        };
        row.insert("Dataset".to_owned(), dataset.clone());
        row.insert("Record".to_owned(), row_value.to_owned());
    }

    rows.sort_by(|left, right| {
        let left_dataset = first(left, &["Dataset", "dataset"]).unwrap_or_default();
        let right_dataset = first(right, &["Dataset", "dataset"]).unwrap_or_default();
        let left_record = first(left, &["Record", "record"]).unwrap_or_default();
        let right_record = first(right, &["Record", "record"]).unwrap_or_default();
        let left_variable = first(left, &["Variable", "variable"]).unwrap_or_default();
        let right_variable = first(right, &["Variable", "variable"]).unwrap_or_default();
        (left_dataset, left_record, left_variable).cmp(&(
            right_dataset,
            right_record,
            right_variable,
        ))
    });
    rows
}

fn normalize_official_core_000007_usubjid_rows(
    rows: Vec<BTreeMap<String, String>>,
    source: ReportSource,
    default_rule_id: Option<&str>,
) -> Vec<BTreeMap<String, String>> {
    if source != ReportSource::Official
        || default_rule_id
            .unwrap_or_default()
            .to_ascii_uppercase()
            .as_str()
            != "CORE-000007"
    {
        return rows;
    }

    let usubjid_by_record = rows
        .iter()
        .filter(|row| {
            first(row, &["Variable", "variable"])
                .is_some_and(|variable| variable.eq_ignore_ascii_case("USUBJID"))
        })
        .filter_map(|row| {
            let record = first(row, &["Record", "record"])?;
            let usubjid = first(row, &["Value", "value"])?;
            Some((record, usubjid))
        })
        .collect::<BTreeMap<_, _>>();
    if usubjid_by_record.is_empty() {
        return rows;
    }

    rows.into_iter()
        .filter_map(|mut row| {
            let record = first(&row, &["Record", "record"])?;
            let usubjid = usubjid_by_record.get(&record)?;
            let variable = first(&row, &["Variable", "variable"]).unwrap_or_default();
            if variable.eq_ignore_ascii_case("USUBJID") {
                return None;
            }
            row.insert("USUBJID".to_owned(), usubjid.clone());
            if let Ok(record_number) = record.parse::<usize>() {
                row.insert("Record".to_owned(), (record_number + 1).to_string());
            }
            Some(row)
        })
        .collect()
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
    let source =
        std::fs::read_to_string(path).with_context(|| format!("read CSV {}", path.display()))?;
    if has_merge_conflict_markers(&source) {
        anyhow::bail!("CSV contains merge conflict markers: {}", path.display());
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(source.as_bytes());
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

fn has_merge_conflict_markers(source: &str) -> bool {
    source.lines().any(|line| {
        line.starts_with("<<<<<<<") || line.starts_with("=======") || line.starts_with(">>>>>>>")
    })
}

fn normalize_row(row: &BTreeMap<String, String>, default_rule_id: Option<&str>) -> IssueKey {
    let rule_id = first(row, &["rule_id", "rule", "core_id", "core-id", "id"])
        .or_else(|| default_rule_id.map(str::to_owned))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let mut dataset = normalize_dataset_like(
        &first(
            row,
            &[
                "dataset",
                "dataset_name",
                "Dataset",
                "domain",
                "domain_name",
            ],
        )
        .unwrap_or_default(),
    );
    let mut domain = normalize_dataset_like(
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
        &rule_id,
    );
    let usubjid = first(row, &["usubjid", "USUBJID", "subject", "subject_id"]).unwrap_or_default();
    let seq = first(row, &["seq", "SEQ", "sequence", "sequence_number"]).unwrap_or_default();
    if variables.as_slice() == ["DATASET_NAME"] && dataset.contains(',') {
        if let Some(value_dataset) = first(row, &["value", "Value"]) {
            dataset = normalize_dataset_like(&value_dataset);
            domain.clone_from(&dataset);
        }
    }

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

fn expand_issue_key(key: IssueKey) -> Vec<IssueKey> {
    if key.variables.len() <= 1 {
        return vec![key];
    }

    key.variables
        .iter()
        .map(|variable| IssueKey {
            variables: vec![variable.clone()],
            ..key.clone()
        })
        .collect()
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

fn split_variables(value: &str, rule_id: &str) -> Vec<String> {
    let value = normalize_scalar(value);
    if value.is_empty() {
        return Vec::new();
    }
    value
        .split(['|', ';', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| normalize_variable_alias(rule_id, &part.to_ascii_uppercase()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_variable_alias(rule_id: &str, variable: &str) -> String {
    match (rule_id, variable) {
        ("CORE-000481", "VSEXFL") => "VSEXCLFL".to_owned(),
        ("CORE-000482", "VSEXFL") => "VSEXCLFL".to_owned(),
        _ => variable.to_owned(),
    }
}

fn row_is_official_dataset_only_blank_marker(row: &BTreeMap<String, String>) -> bool {
    let has_dataset = row.iter().any(|(key, value)| {
        key.trim().eq_ignore_ascii_case("Dataset") && !normalize_scalar(value).is_empty()
    });
    has_dataset
        && row.iter().all(|(key, value)| {
            key.trim().eq_ignore_ascii_case("Dataset")
                || key.trim().eq_ignore_ascii_case("domain")
                || normalize_scalar(value).is_empty()
        })
}

fn row_is_core_rs_skipped(row: &BTreeMap<String, String>) -> bool {
    let status = first(row, &["execution_status", "status"]).unwrap_or_default();
    let skipped_reason = first(row, &["skipped_reason", "skip_reason"]).unwrap_or_default();
    status.eq_ignore_ascii_case("skipped") || !skipped_reason.is_empty()
}

fn collect_core_rs_skipped_reasons(rows: &[BTreeMap<String, String>]) -> BTreeMap<String, usize> {
    let mut reasons = BTreeMap::new();
    for row in rows.iter().filter(|row| row_is_core_rs_skipped(row)) {
        let reason = first(row, &["skipped_reason", "skip_reason"])
            .unwrap_or_else(|| "unspecified".to_owned());
        *reasons.entry(reason).or_default() += 1;
    }
    reasons
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

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000001"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000001"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
        assert_eq!(official.issues[0].dataset, "CM");
        assert_eq!(official.issues[0].variables, vec!["CMSEQ"]);
        assert_eq!(official.issues[1].variables, vec!["CMTRT"]);
    }

    #[test]
    fn normalizes_known_open_rules_variable_aliases() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(&official, "Dataset,Record,Variable\nVS,3,VSEXFL\n").expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000481,failed,VS,VS,3,VSEXCLFL,text,1,,,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000481"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000481"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn normalizes_send_exclusion_flag_aliases() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(&official, "Dataset,Record,Variable\nVS,4,VSEXFL\n").expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000482,failed,VS,VS,4,VSEXCLFL,text,1,,,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000482"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000482"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn normalizes_official_dataset_name_list_rows_to_value_dataset() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,Value\n\
\"SUPPQSCQI.CSV, SUPPQSSWLS\",1,dataset_name,SUPPQSCQI\n\
\"SUPPQSCQI.CSV, SUPPQSSWLS\",1,dataset_name,SUPPQSSWLS\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000357,failed,SUPPQSCQI,SUPPQSCQI,1,dataset_name,text,1,,,\n\
CORE-000357,failed,SUPPQSSWLS,SUPPQSSWLS,1,dataset_name,text,1,,,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000357"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000357"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn normalizes_core_000539_dataset_name_operation_pairs() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,Value\n\
QS1,1,dataset_name,QS1\n\
QS1,1,$list_dataset_names,\"['QS1', 'QSAE']\"\n\
QS1,2,dataset_name,QSAE\n\
QS1,2,$list_dataset_names,\"['QS1', 'QSAE']\"\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000539,failed,QS1,QS1,1,dataset_name|$list_dataset_names,text,1,,,\n\
CORE-000539,failed,QSAE,QSAE,1,dataset_name|$list_dataset_names,text,1,,,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000539"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000539"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn normalizes_core_000540_dataset_name_operation_rows_to_dataset_level() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,Value\n\
FACM,1,dataset_name,FACM\n\
FACM,1,$list_dataset_names,['FACM']\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000540,failed,FACM,FACM,,dataset_name|$list_dataset_names,text,1,,,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000540"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000540"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn normalizes_core_000007_official_usubjid_variable_rows() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,Value\n\
DM,3,DTHDTC,2018-06-10\n\
DM,3,DTHFL,\n\
DM,3,USUBJID,015246-099-0000-00002\n\
DM,4,DTHDTC,2018-09-04\n\
DM,4,DTHFL,N\n\
DM,4,USUBJID,015246-099-0000-00003\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
CORE-000007,failed,DM,DM,4,DTHDTC|DTHFL,text,2,,015246-099-0000-00002,\n\
CORE-000007,failed,DM,DM,5,DTHDTC|DTHFL,text,2,,015246-099-0000-00003,\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000007"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000007"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
    }

    #[test]
    fn expands_multi_variable_rows_to_variable_level_issue_keys() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("report.csv");
        fs::write(
            &report,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,failed,IE,IE,1,IECAT|IEORRES,text,2,,,\n",
        )
        .expect("write report");

        let normalized = normalize_csv(&report, ReportSource::CoreRs, None).expect("normalize");

        assert_eq!(normalized.issue_count, 2);
        assert_eq!(normalized.issues[0].variables, vec!["IECAT"]);
        assert_eq!(normalized.issues[1].variables, vec!["IEORRES"]);
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
        assert_eq!(
            normalized.skipped_reasons,
            BTreeMap::from([("unsupported_operator".to_owned(), 1)])
        );
        assert_eq!(normalized.issue_count, 0);
        assert!(normalized.issues.is_empty());
    }

    #[test]
    fn official_dataset_only_blank_rows_are_not_issue_keys() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("results.csv");
        fs::write(
            &report,
            "Dataset,Record,Variable,Value\nLB,,,\nDS,2,DSDY,0\n",
        )
        .expect("write report");

        let normalized =
            normalize_csv(&report, ReportSource::Official, Some("CORE-000249")).expect("normalize");

        assert_eq!(normalized.row_count, 2);
        assert_eq!(normalized.issue_count, 1);
        assert_eq!(normalized.issues[0].dataset, "DS");
        assert_eq!(normalized.issues[0].row, "2");
        assert_eq!(normalized.issues[0].variables, vec!["DSDY"]);
    }

    #[test]
    fn core_rs_collects_distinct_skipped_reasons() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("report.csv");
        fs::write(
            &report,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\n\
             CORE-000001,skipped,,,,,unsupported,0,unsupported_operator,,\n\
             CORE-000001,skipped,,,,,unsupported,0,unsupported_operator,,\n\
             CORE-000001,skipped,,,,,join,0,dataset_join_not_supported,,\n",
        )
        .expect("write report");

        let normalized = normalize_csv(&report, ReportSource::CoreRs, None).expect("normalize");

        assert_eq!(normalized.skipped_row_count, 3);
        assert_eq!(
            normalized.skipped_reasons,
            BTreeMap::from([
                ("dataset_join_not_supported".to_owned(), 1),
                ("unsupported_operator".to_owned(), 2),
            ])
        );
    }

    #[test]
    fn official_merge_conflict_marker_rows_are_rejected() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("results.csv");
        fs::write(
            &report,
            "Dataset,Record,Variable,Value\n<<<<<<< HEAD\nLB,0,LBTESTCD,OTHER\n=======\nLB.csv,1,LBTESTCD,OTHER\n>>>>>>> main\n",
        )
        .expect("write report");

        let error = normalize_csv(&report, ReportSource::Official, Some("CORE-000159"))
            .expect_err("conflict markers should fail normalization");

        assert!(error.to_string().contains("merge conflict markers"));
    }

    #[test]
    fn nullish_values_normalize_to_empty_but_zero_and_dot_remain() {
        assert_eq!(normalize_scalar(" null "), "");
        assert_eq!(normalize_scalar("N/A"), "");
        assert_eq!(normalize_scalar("0"), "0");
        assert_eq!(normalize_scalar("."), ".");
    }
}
