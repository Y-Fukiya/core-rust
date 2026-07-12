use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::OpenRulesCase;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvenance {
    NativeEngine,
    RuleIdHandPort,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvenanceDetail {
    GenericEngine,
    RuleSpecificEngineSemantics,
    CompatibilityPolicy,
    RuleIdHandPort,
    // Retained for backward-compatible baseline deserialization. New
    // scoreboards report normalization through ScoringPolicy instead.
    OracleGapNormalized,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ScoringPolicy {
    #[default]
    StrictIdentity,
    OracleGapNormalized,
}

impl ExecutionProvenance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NativeEngine => "native_engine",
            Self::RuleIdHandPort => "rule_id_hand_port",
            Self::Unknown => "unknown",
        }
    }
}

impl ExecutionProvenanceDetail {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GenericEngine => "generic_engine",
            Self::RuleSpecificEngineSemantics => "rule_specific_engine_semantics",
            Self::CompatibilityPolicy => "compatibility_policy",
            Self::RuleIdHandPort => "rule_id_hand_port",
            Self::OracleGapNormalized => "oracle_gap_normalized",
            Self::Unknown => "unknown",
        }
    }
}

impl ScoringPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StrictIdentity => "strict_identity",
            Self::OracleGapNormalized => "oracle_gap_normalized",
        }
    }
}

pub fn candidate_execution_provenance(case: &OpenRulesCase, path: &Path) -> ExecutionProvenance {
    read_candidate_execution_provenance(path)
        .or_else(|| read_candidate_execution_provenance_json(&path.with_extension("json")))
        .unwrap_or_else(|| execution_provenance_for_rule_id(&case.rule_id))
}

pub fn execution_provenance_for_rule_id(rule_id: &str) -> ExecutionProvenance {
    if core_api::rule_id_uses_hand_port(rule_id) {
        ExecutionProvenance::RuleIdHandPort
    } else {
        ExecutionProvenance::NativeEngine
    }
}

pub fn execution_provenance_detail_for_case(
    rule_id: &str,
    provenance: &ExecutionProvenance,
    _scoring_normalizations: &[String],
) -> ExecutionProvenanceDetail {
    match provenance {
        ExecutionProvenance::RuleIdHandPort => ExecutionProvenanceDetail::RuleIdHandPort,
        ExecutionProvenance::Unknown => ExecutionProvenanceDetail::Unknown,
        ExecutionProvenance::NativeEngine => {
            match core_api::rule_id_specific_semantics_classification(rule_id) {
                Some("compatibility_policy") => ExecutionProvenanceDetail::CompatibilityPolicy,
                Some(_) => ExecutionProvenanceDetail::RuleSpecificEngineSemantics,
                None => ExecutionProvenanceDetail::GenericEngine,
            }
        }
    }
}

pub fn scoring_policy_for_normalizations(scoring_normalizations: &[String]) -> ScoringPolicy {
    if scoring_normalizations.is_empty() {
        ScoringPolicy::StrictIdentity
    } else {
        ScoringPolicy::OracleGapNormalized
    }
}

fn read_candidate_execution_provenance(path: &Path) -> Option<ExecutionProvenance> {
    let source = std::fs::read_to_string(path).ok()?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(source.as_bytes());
    let headers = reader.headers().ok()?.clone();
    let index = headers
        .iter()
        .position(|header| header.trim().eq_ignore_ascii_case("execution_provenance"))?;
    for row in reader.records().flatten() {
        let value = row.get(index).unwrap_or_default().trim();
        if value.is_empty() {
            continue;
        }
        return Some(execution_provenance_from_label(value));
    }
    None
}

fn read_candidate_execution_provenance_json(path: &Path) -> Option<ExecutionProvenance> {
    let source = std::fs::read_to_string(path).ok()?;
    let document: serde_json::Value = serde_json::from_str(&source).ok()?;
    let results = document.get("results")?.as_array()?;
    let labels = results
        .iter()
        .map(|result| {
            result
                .get("execution_provenance")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .collect::<Vec<_>>();
    let explicit_count = labels.iter().filter(|label| label.is_some()).count();
    if explicit_count == 0 {
        return None;
    }
    if explicit_count != results.len() {
        return Some(ExecutionProvenance::Unknown);
    }
    let mut provenances = labels
        .into_iter()
        .flatten()
        .map(execution_provenance_from_label);
    let first = provenances.next()?;
    if provenances.all(|provenance| provenance == first) {
        Some(first)
    } else {
        Some(ExecutionProvenance::Unknown)
    }
}

fn execution_provenance_from_label(value: &str) -> ExecutionProvenance {
    match value.trim().to_ascii_lowercase().as_str() {
        "native_engine"
        | "generic_engine"
        | "rule_specific_engine_semantics"
        | "compatibility_policy" => ExecutionProvenance::NativeEngine,
        "rule_id_hand_port" => ExecutionProvenance::RuleIdHandPort,
        _ => ExecutionProvenance::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::open_rules::discovery::CaseKind;
    use tempfile::tempdir;

    #[test]
    fn detailed_native_provenance_values_map_to_native_engine() {
        for value in [
            "native_engine",
            "generic_engine",
            "rule_specific_engine_semantics",
            "compatibility_policy",
        ] {
            let dir = tempdir().expect("tempdir");
            let path = dir.path().join("report.csv");
            std::fs::write(&path, format!("execution_provenance\n{value}\n"))
                .expect("write report");
            assert_eq!(
                read_candidate_execution_provenance(&path),
                Some(ExecutionProvenance::NativeEngine)
            );
        }
    }

    #[test]
    fn unknown_provenance_value_remains_unknown() {
        assert_eq!(
            execution_provenance_from_label("engine_semantics"),
            ExecutionProvenance::Unknown
        );
    }

    #[test]
    fn json_report_supplies_provenance_without_changing_csv_schema() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.json");
        std::fs::write(
            &path,
            r#"{"results":[{"execution_provenance":"compatibility_policy"}]}"#,
        )
        .expect("write report");
        assert_eq!(
            read_candidate_execution_provenance_json(&path),
            Some(ExecutionProvenance::NativeEngine)
        );
    }

    #[test]
    fn mixed_json_report_provenance_is_unknown() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.json");
        std::fs::write(
            &path,
            r#"{"results":[
                {"execution_provenance":"generic_engine"},
                {"execution_provenance":"rule_id_hand_port"}
            ]}"#,
        )
        .expect("write report");
        assert_eq!(
            read_candidate_execution_provenance_json(&path),
            Some(ExecutionProvenance::Unknown)
        );
    }

    #[test]
    fn explicit_and_missing_json_provenance_is_unknown() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.json");
        std::fs::write(
            &path,
            r#"{"results":[
                {"execution_provenance":"generic_engine"},
                {"execution_provenance":null},
                {}
            ]}"#,
        )
        .expect("write report");
        assert_eq!(
            read_candidate_execution_provenance_json(&path),
            Some(ExecutionProvenance::Unknown)
        );
    }

    #[test]
    fn entirely_missing_json_provenance_uses_fallback() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("report.json");
        std::fs::write(&path, r#"{"results":[{"execution_provenance":""},{}]}"#)
            .expect("write report");
        assert_eq!(read_candidate_execution_provenance_json(&path), None);
    }

    #[test]
    fn candidate_provenance_uses_json_sidecar_before_rule_id_fallback() {
        let dir = tempdir().expect("tempdir");
        let csv_path = dir.path().join("report.csv");
        std::fs::write(&csv_path, "rule_id,execution_status\nCORE-000583,passed\n")
            .expect("write csv report");
        std::fs::write(
            csv_path.with_extension("json"),
            r#"{"results":[{"execution_provenance":"generic_engine"}]}"#,
        )
        .expect("write json report");
        let case = OpenRulesCase {
            scope: "Published".to_owned(),
            rule_id: "CORE-000583".to_owned(),
            rule_dir: PathBuf::new(),
            rule_path: PathBuf::new(),
            case_kind: CaseKind::Negative,
            case_id: "01".to_owned(),
            case_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
            env_path: PathBuf::new(),
            env: BTreeMap::new(),
            datasets_path: PathBuf::new(),
            datasets: Vec::new(),
            dataset_files: Vec::new(),
            variables_path: PathBuf::new(),
            variables: Vec::new(),
            official_results_csv: PathBuf::new(),
            has_official_results: true,
        };

        assert_eq!(
            candidate_execution_provenance(&case, &csv_path),
            ExecutionProvenance::NativeEngine
        );
    }
}
