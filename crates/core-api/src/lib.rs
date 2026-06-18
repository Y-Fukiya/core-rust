#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use core_data::{left_join_dataset, load_datasets_from_paths, DataError, LoadedDataset};
use core_engine::{validate_rule, EngineError, RuleValidationResult, SkippedReason};
use core_report::{write_reports, ReportError, WrittenReports};
use core_rule_model::{
    load_rules_from_paths, ConditionGroup, ExecutableRule, OperationSpec, Operator, RuleModelError,
    RuleType, Sensitivity,
};
use serde_json::Value;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("--rules and --exclude-rules cannot be used together")]
    MutuallyExclusiveRuleFilters,
    #[error("at least one rule path is required")]
    MissingRulePaths,
    #[error("at least one dataset path is required")]
    MissingDatasetPaths,
    #[error("failed to load rules: {0}")]
    RuleLoad(#[from] RuleModelError),
    #[error("failed to load datasets: {0}")]
    DataLoad(#[from] DataError),
    #[error("failed to validate rule: {0}")]
    Engine(#[from] EngineError),
    #[error("failed to write reports: {0}")]
    Report(#[from] ReportError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidateRequest {
    pub rule_paths: Vec<PathBuf>,
    pub dataset_paths: Vec<PathBuf>,
    pub include_rules: Vec<String>,
    pub exclude_rules: Vec<String>,
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ValidateOutcome {
    pub results: Vec<RuleValidationResult>,
    pub reports: Option<WrittenReports>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSelection {
    pub selected: Vec<ExecutableRule>,
    pub skipped: Vec<RuleValidationResult>,
}

pub fn run_validation(request: ValidateRequest) -> Result<ValidateOutcome> {
    if !request.include_rules.is_empty() && !request.exclude_rules.is_empty() {
        return Err(ApiError::MutuallyExclusiveRuleFilters);
    }
    if request.rule_paths.is_empty() {
        return Err(ApiError::MissingRulePaths);
    }
    if request.dataset_paths.is_empty() {
        return Err(ApiError::MissingDatasetPaths);
    }

    let rules = load_rules_from_paths(&request.rule_paths)?;
    let datasets = load_datasets_from_paths(&request.dataset_paths)?;
    let selection = select_rules(&rules, &request.include_rules, &request.exclude_rules)?;

    let mut results = selection.skipped;
    for rule in &selection.selected {
        if let Some(skipped) = skipped_unsupported_rule(rule) {
            results.push(skipped);
            continue;
        }

        let execution_datasets = match execution_datasets_for_rule(rule, &datasets) {
            Ok(datasets) => datasets,
            Err(skipped) => {
                results.push(skipped);
                continue;
            }
        };

        for dataset in &execution_datasets {
            results.push(validate_rule(rule, dataset)?);
        }
    }

    let reports = request
        .output_dir
        .map(|output_dir| write_reports(output_dir, &results))
        .transpose()?;

    Ok(ValidateOutcome { results, reports })
}

pub fn select_rules(
    rules: &[ExecutableRule],
    include_rules: &[String],
    exclude_rules: &[String],
) -> Result<RuleSelection> {
    if !include_rules.is_empty() && !exclude_rules.is_empty() {
        return Err(ApiError::MutuallyExclusiveRuleFilters);
    }

    let available_ids: BTreeSet<&str> = rules.iter().map(|rule| rule.core_id.as_str()).collect();
    let selected = if include_rules.is_empty() {
        rules
            .iter()
            .filter(|rule| !exclude_rules.iter().any(|id| id == &rule.core_id))
            .cloned()
            .collect()
    } else {
        include_rules
            .iter()
            .filter_map(|id| rules.iter().find(|rule| rule.core_id == *id).cloned())
            .collect()
    };

    let filter_ids = if include_rules.is_empty() {
        exclude_rules
    } else {
        include_rules
    };
    let skipped = missing_rule_ids(filter_ids, &available_ids)
        .into_iter()
        .map(|id| {
            RuleValidationResult::skipped_rule(
                id.clone(),
                SkippedReason::RuleNotFound,
                format!("Requested rule {id} was not found"),
            )
        })
        .collect();

    Ok(RuleSelection { selected, skipped })
}

fn skipped_unsupported_rule(rule: &ExecutableRule) -> Option<RuleValidationResult> {
    if !matches!(rule.rule_type, RuleType::RecordData | RuleType::Jsonata) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!(
                "Rule {} has unsupported rule type {}",
                rule.core_id,
                rule.rule_type.as_name()
            ),
        ));
    }

    if !matches!(
        rule.sensitivity,
        Some(Sensitivity::Record | Sensitivity::Dataset)
    ) {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedRuleType,
            format!("Rule {} has unsupported sensitivity", rule.core_id),
        ));
    }

    if !rule.operations.is_empty() && supported_join_operation(rule).is_none() {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::OperationsNotSupported,
            format!(
                "Rule {} uses Operations, which are not supported",
                rule.core_id
            ),
        ));
    }

    if rule
        .datasets
        .as_ref()
        .is_some_and(|datasets| !datasets.is_empty())
        && supported_join_operation(rule).is_none()
    {
        return Some(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::DatasetJoinNotSupported,
            format!(
                "Rule {} uses Match Datasets, which are not supported",
                rule.core_id
            ),
        ));
    }

    unsupported_operator(&rule.conditions).map(|operator| {
        RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::UnsupportedOperator,
            format!(
                "Rule {} uses unsupported operator {}",
                rule.core_id,
                operator.as_name()
            ),
        )
    })
}

fn execution_datasets_for_rule(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some(operation) = supported_join_operation(rule) else {
        return Ok(datasets.to_vec());
    };

    let Some(keys) = string_array_field(operation, &["by", "keys", "on"]) else {
        return Err(join_skipped_result(rule, "join operation is missing keys"));
    };
    let Some(left_name) = string_field(operation, &["left", "primary", "dataset"]) else {
        return Err(join_skipped_result(
            rule,
            "join operation is missing left dataset",
        ));
    };
    let Some(right_name) = string_field(operation, &["right", "with", "secondary"]) else {
        return Err(join_skipped_result(
            rule,
            "join operation is missing right dataset",
        ));
    };

    let Some(left) = find_dataset(datasets, &left_name) else {
        return Err(join_skipped_result(
            rule,
            format!("left dataset {left_name} was not loaded"),
        ));
    };
    let Some(right) = find_dataset(datasets, &right_name) else {
        return Err(join_skipped_result(
            rule,
            format!("right dataset {right_name} was not loaded"),
        ));
    };

    let prefix =
        string_field(operation, &["prefix"]).unwrap_or_else(|| format!("{}.", right.metadata.name));
    left_join_dataset(left, right, &keys, &prefix)
        .map(|dataset| vec![dataset])
        .map_err(|source| join_skipped_result(rule, source.to_string()))
}

fn join_skipped_result(rule: &ExecutableRule, message: impl Into<String>) -> RuleValidationResult {
    RuleValidationResult::skipped_rule(
        rule.core_id.clone(),
        SkippedReason::DatasetJoinNotSupported,
        format!(
            "Rule {} cannot run dataset join: {}",
            rule.core_id,
            message.into()
        ),
    )
}

fn supported_join_operation(rule: &ExecutableRule) -> Option<&OperationSpec> {
    rule.operations.iter().find(|operation| {
        matches!(
            operation_name(operation).as_deref(),
            Some("join" | "left_join" | "dataset_join")
        )
    })
}

fn operation_name(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["name", "type", "operation"]).map(|value| {
        value
            .trim()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .split('_')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    })
}

fn string_field(operation: &OperationSpec, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_case_insensitive(operation, key))
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn string_array_field(operation: &OperationSpec, keys: &[&str]) -> Option<Vec<String>> {
    keys.iter()
        .find_map(|key| {
            operation
                .fields
                .get(*key)
                .or_else(|| field_case_insensitive(operation, key))
        })
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
}

fn field_case_insensitive<'a>(operation: &'a OperationSpec, key: &str) -> Option<&'a Value> {
    operation
        .fields
        .iter()
        .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
        .map(|(_key, value)| value)
}

fn find_dataset<'a>(datasets: &'a [LoadedDataset], name: &str) -> Option<&'a LoadedDataset> {
    datasets.iter().find(|dataset| {
        dataset.metadata.name.eq_ignore_ascii_case(name)
            || dataset
                .metadata
                .domain
                .as_deref()
                .is_some_and(|domain| domain.eq_ignore_ascii_case(name))
            || dataset.metadata.filename.eq_ignore_ascii_case(name)
    })
}

fn unsupported_operator(group: &ConditionGroup) -> Option<&Operator> {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            groups.iter().find_map(unsupported_operator)
        }
        ConditionGroup::Not(group) => unsupported_operator(group),
        ConditionGroup::Leaf(condition) => {
            (!is_supported_basic_operator(&condition.operator)).then_some(&condition.operator)
        }
    }
}

fn is_supported_basic_operator(operator: &Operator) -> bool {
    matches!(
        operator,
        Operator::Exists
            | Operator::NotExists
            | Operator::EqualTo
            | Operator::NotEqualTo
            | Operator::EqualToCaseInsensitive
            | Operator::NotEqualToCaseInsensitive
            | Operator::Contains
            | Operator::DoesNotContain
            | Operator::ContainsCaseInsensitive
            | Operator::DoesNotContainCaseInsensitive
            | Operator::IsContainedBy
            | Operator::IsNotContainedBy
            | Operator::IsContainedByCaseInsensitive
            | Operator::IsNotContainedByCaseInsensitive
            | Operator::LessThan
            | Operator::LessThanOrEqualTo
            | Operator::GreaterThan
            | Operator::GreaterThanOrEqualTo
            | Operator::MatchesRegex
            | Operator::DoesNotMatchRegex
            | Operator::IsEmpty
            | Operator::IsNotEmpty
    )
}

fn missing_rule_ids<'a>(
    requested: &'a [String],
    available_ids: &BTreeSet<&str>,
) -> Vec<&'a String> {
    let mut seen = BTreeSet::new();
    requested
        .iter()
        .filter(|id| seen.insert(id.as_str()) && !available_ids.contains(id.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use core_engine::ExecutionStatus;
    use core_rule_model::{load_rules_from_paths, Sensitivity};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    fn write_rule(dir: &std::path::Path, id: &str, expected_domain: &str) {
        fs::write(
            dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{}}, "Classes": {{}} }},
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {{
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "{expected_domain}"
  }},
  "Outcome": {{ "Message": "DOMAIN must be {expected_domain}" }}
}}"#
            ),
        )
        .expect("write rule");
    }

    fn write_dataset(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("datasets.json");
        fs::write(
            &path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2],
        "DOMAIN": ["AE", "CM"]
      }
    }
  ]
}"#,
        )
        .expect("write dataset");
        path
    }

    #[test]
    fn select_rules_includes_only_requested_ids_and_skips_missing_ids() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        write_rule(dir.path(), "CORE-TEST-0002", "CM");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        let selection = select_rules(
            &rules,
            &["CORE-TEST-0002".to_owned(), "CORE-MISSING".to_owned()],
            &[],
        )
        .expect("select rules");

        assert_eq!(selection.selected.len(), 1);
        assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
        assert_eq!(
            selection.skipped[0].execution_status,
            ExecutionStatus::Skipped
        );
    }

    #[test]
    fn select_rules_excludes_requested_ids_and_skips_missing_exclusions() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        write_rule(dir.path(), "CORE-TEST-0002", "CM");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        let selection = select_rules(
            &rules,
            &[],
            &["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
        )
        .expect("select rules");

        assert_eq!(selection.selected.len(), 1);
        assert_eq!(selection.selected[0].core_id, "CORE-TEST-0002");
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].rule_id, "CORE-MISSING");
    }

    #[test]
    fn select_rules_rejects_include_and_exclude_together() {
        let error = select_rules(
            &[],
            &["CORE-TEST-0001".to_owned()],
            &["CORE-TEST-0002".to_owned()],
        )
        .expect_err("mutually exclusive filters");

        assert!(matches!(error, ApiError::MutuallyExclusiveRuleFilters));
    }

    #[test]
    fn run_validation_filters_rules_and_writes_reports() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        let output_dir = dir.path().join("out");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        write_rule(&rules_dir, "CORE-TEST-0001", "AE");
        write_rule(&rules_dir, "CORE-TEST-0002", "CM");
        let dataset_path = write_dataset(&data_dir);

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: vec!["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
            exclude_rules: Vec::new(),
            output_dir: Some(output_dir.clone()),
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Skipped
        );
        assert_eq!(outcome.results[0].rule_id, "CORE-MISSING");
        assert_eq!(outcome.results[1].rule_id, "CORE-TEST-0001");
        assert_eq!(outcome.results[1].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[1].error_count, 1);
        assert!(outcome.reports.expect("reports").json.exists());
        assert!(output_dir.join("report.csv").exists());
    }

    #[test]
    fn run_validation_requires_paths_before_loading() {
        let request = ValidateRequest {
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
            rule_paths: Vec::new(),
            dataset_paths: Vec::new(),
        };

        let error = run_validation(request).expect_err("missing rule paths");
        assert!(matches!(error, ApiError::MissingRulePaths));
    }

    #[test]
    fn loaded_rules_keep_record_sensitivity() {
        let dir = tempdir().expect("tempdir");
        write_rule(dir.path(), "CORE-TEST-0001", "AE");
        let rules = load_rules_from_paths(&[dir.path().to_path_buf()]).expect("load rules");

        assert_eq!(rules[0].sensitivity, Some(Sensitivity::Record));
    }

    #[test]
    fn run_validation_skips_unsupported_rules_before_engine_execution() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        write_raw_rule(
            &rules_dir,
            "CORE-OPERATIONS",
            r#""Rule Type": "Record Data""#,
            r#""Operations": [{ "name": "aggregate" }],"#,
            r#""operator": "equal_to""#,
        );
        write_raw_rule(
            &rules_dir,
            "CORE-JOIN",
            r#""Rule Type": "Record Data""#,
            r#""Match Datasets": [{ "domain": "SUPPAE" }],"#,
            r#""operator": "equal_to""#,
        );
        write_raw_rule(
            &rules_dir,
            "CORE-OPERATOR",
            r#""Rule Type": "Record Data""#,
            "",
            r#""operator": "future_operator""#,
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 3);
        let reasons = outcome
            .results
            .iter()
            .map(|result| result.skipped_reason.as_ref().expect("skipped reason"))
            .map(|reason| serde_json::to_string(reason).expect("serialize reason"))
            .map(|reason| reason.trim_matches('"').to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            reasons,
            BTreeSet::from([
                "dataset_join_not_supported".to_owned(),
                "operations_not_supported".to_owned(),
                "unsupported_operator".to_owned(),
            ])
        );
        assert!(outcome
            .results
            .iter()
            .all(|result| result.execution_status == ExecutionStatus::Skipped));
    }

    #[test]
    fn run_validation_executes_jsonata_rules_when_conditions_are_normalized() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");
        let dataset_path = write_dataset(&data_dir);

        write_raw_rule(
            &rules_dir,
            "CORE-JSONATA",
            r#""Rule Type": "JSONATA""#,
            "",
            r#""operator": "not_equal_to""#,
        );

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].rule_id, "CORE-JSONATA");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
    }

    #[test]
    fn run_validation_executes_supported_dataset_join_operation() {
        let dir = tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        let data_dir = dir.path().join("data");
        fs::create_dir_all(&rules_dir).expect("rules dir");
        fs::create_dir_all(&data_dir).expect("data dir");

        fs::write(
            rules_dir.join("CORE-JOIN-SUPP.json"),
            r#"{
  "Core": { "Id": "CORE-JOIN-SUPP", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [{ "domain": "AE" }, { "domain": "SUPPAE" }],
  "Operations": [
    {
      "name": "left_join",
      "left": "AE",
      "right": "SUPPAE",
      "by": ["USUBJID"],
      "prefix": "SUPP."
    }
  ],
  "Check": {
    "name": "SUPP.QVAL",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "SUPPAE QVAL must not be BAD" }
}"#,
        )
        .expect("write join rule");

        let dataset_path = data_dir.join("datasets.json");
        fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DOMAIN": ["AE", "AE"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S2"],
        "QNAM": ["AESPID"],
        "QVAL": ["BAD"]
      }
    }
  ]
}"#,
        )
        .expect("write join data");

        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir],
            dataset_paths: vec![dataset_path],
            include_rules: Vec::new(),
            exclude_rules: Vec::new(),
            output_dir: None,
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].row, Some(2));
        assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    }

    fn write_raw_rule(
        dir: &std::path::Path,
        id: &str,
        rule_type: &str,
        extra_rule_field: &str,
        operator: &str,
    ) {
        fs::write(
            dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{}}, "Classes": {{}} }},
  "Sensitivity": "Record",
  {rule_type},
  {extra_rule_field}
  "Check": {{
    "name": "DOMAIN",
    {operator},
    "value": "AE"
  }},
  "Outcome": {{ "Message": "DOMAIN must be AE" }}
}}"#
            ),
        )
        .expect("write raw rule");
    }
}
