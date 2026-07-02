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
    OracleGapNormalized,
    #[default]
    Unknown,
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

pub fn candidate_execution_provenance(case: &OpenRulesCase, path: &Path) -> ExecutionProvenance {
    read_candidate_execution_provenance(path)
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
    scoring_normalizations: &[String],
) -> ExecutionProvenanceDetail {
    if !scoring_normalizations.is_empty() {
        return ExecutionProvenanceDetail::OracleGapNormalized;
    }
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
        return Some(match value.to_ascii_lowercase().as_str() {
            "native_engine" => ExecutionProvenance::NativeEngine,
            "rule_id_hand_port" => ExecutionProvenance::RuleIdHandPort,
            _ => ExecutionProvenance::Unknown,
        });
    }
    None
}
