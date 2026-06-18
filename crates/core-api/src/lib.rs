#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use core_data::{load_datasets_from_paths, DataError};
use core_engine::{validate_rule, EngineError, RuleValidationResult, SkippedReason};
use core_report::{write_reports, ReportError, WrittenReports};
use core_rule_model::{load_rules_from_paths, ExecutableRule, RuleModelError};
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
        for dataset in &datasets {
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
    use std::fs;

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
}
