
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
fn preflight_accepts_is_not_unique_relationship_operator() {
    assert!(is_supported_basic_operator(
        &Operator::IsNotUniqueRelationship
    ));
}

#[test]
fn run_validation_uses_open_rules_data_loader_when_requested() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("create rules dir");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::write(
        rules_dir.join("CORE-OPEN-0001.yml"),
        r#"Core:
  Id: CORE-OPEN-0001
  Status: Published
Scope:
  Domains: {}
  Classes: {}
Sensitivity: Record
Rule Type: Record Data
Check:
  name: CMSEQ
  operator: less_than_or_equal_to
  value: 0
Outcome:
  Message: CMSEQ must be greater than zero
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\ncm,Concomitant Medications\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(data_dir.join("cm.csv"), "CMSEQ\n001\n").expect("write dataset csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
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
        dataset_paths: vec![dataset_path.clone()],
        include_rules: vec!["CORE-TEST-0001".to_owned(), "CORE-MISSING".to_owned()],
        exclude_rules: Vec::new(),
        output_dir: Some(output_dir.clone()),
        ..Default::default()
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
    assert!(outcome
        .reports
        .expect("reports")
        .json
        .expect("json report")
        .exists());
    assert!(output_dir.join("report.csv").exists());
}

#[test]
fn run_validation_filters_execution_datasets_by_domain_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-DOMAIN-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["MS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "MS",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must not be MS" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "ms.xpt",
      "domain": "MS",
      "records": { "USUBJID": ["S1"], "MSSEQ": [1], "DOMAIN": ["MS"] }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "AE");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
}

#[test]
fn run_validation_domain_scope_matches_supp_placeholder_domains() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SUPP-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-SUPP-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "QNAM",
    "operator": "matches_regex",
    "value": "^[0-9]"
  },
  "Outcome": { "Message": "QNAM starts with a number" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "supplb.xpt",
      "domain": "SUPPLB",
      "records": {
        "USUBJID": ["S1"],
        "IDVAR": ["LBSEQ"],
        "IDVARVAL": ["1"],
        "QNAM": ["5BIOSIG"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "SUPPLB");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_filters_execution_datasets_by_class_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-CLASS-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-CLASS-SCOPE", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "equal_to",
    "value": "LB",
    "value_is_literal": true
  },
  "Outcome": { "Message": "DOMAIN must be LB" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "USUBJID": ["S1"], "AESEQ": [1], "DOMAIN": ["AE"] }
    },
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": { "USUBJID": ["S1"], "LBSEQ": [1], "DOMAIN": ["LB"] }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": { "USUBJID": ["S1"], "FASEQ": [1], "DOMAIN": ["FA"] }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert_eq!(outcome.results[0].dataset, "LB");
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[1].dataset, "FA");
    assert_eq!(
        outcome.results[1].execution_status,
        ExecutionStatus::Passed,
        "{:?}",
        outcome.results[1]
    );
}

#[test]
fn run_validation_loads_xpt_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_rule(&rules_dir, "CORE-XPT-0001", "AE");
    let dataset_path = data_dir.join("ae.xpt");
    write_test_xpt_char_dataset(
        &dataset_path,
        "AE",
        &["STUDYID", "DOMAIN", "AESEQ"],
        &[vec!["CDISC-TEST", "AE", "1"], vec!["CDISC-TEST", "CM", "2"]],
    );

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_records_engine_errors_as_skipped_results() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-MISSING-COLUMN.json"),
        r#"{
  "Core": { "Id": "CORE-MISSING-COLUMN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESTDTC",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "AESTDTC must be populated" }
}"#,
    )
    .expect("write missing column rule");
    write_rule(&rules_dir, "CORE-VALID", "AE");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let skipped = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-MISSING-COLUMN")
        .expect("skipped missing column result");
    assert_eq!(skipped.execution_status, ExecutionStatus::Skipped);
    assert_eq!(skipped.skipped_reason, Some(SkippedReason::EvaluationError));
    assert_eq!(skipped.dataset, "AE");
    assert!(skipped
        .message
        .contains("dataset is missing required column"));

    let valid = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-VALID")
        .expect("valid rule result");
    assert_eq!(valid.execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_safe_open_rules_missing_columns_as_null() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000200.json"),
        r#"{
  "Core": { "Id": "CORE-000200", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--STAT", "operator": "empty" },
          { "name": "--DRVFL", "operator": "not_equal_to", "value": "Y", "value_is_literal": true },
          { "name": "--ORRES", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "--ORRES cannot be null" }
}"#,
    )
    .expect("write open rules missing-column rule");
    let dataset_path = data_dir.join("lb.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1"],
        "LBSEQ": [1],
        "LBSTAT": [""],
        "LBORRES": ["12"]
      }
    }
  ]
}"#,
    )
    .expect("write open rules data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_treats_safe_usdm_missing_nested_columns_as_null() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000680.json"),
        r#"{
  "Core": { "Id": "CORE-000680", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Range"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Range" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel", "operator": "is_contained_by", "value": ["plannedCompletionNumber"] },
      {
        "not": {
          "any": [
            { "name": "unit", "operator": "equal_to", "value": false },
            { "name": "unit", "operator": "empty" },
            { "name": "unit", "operator": "not_exists" }
          ]
        }
      }
    ]
  },
  "Outcome": { "Message": "A unit is specified" }
}"#,
    )
    .expect("write usdm missing-column rule");
    let dataset_path = data_dir.join("range.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Range.csv",
      "domain": "Range",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["StudyDesignPopulation_2"],
        "parent_rel": ["plannedCompletionNumber"],
        "rel_type": ["definition"],
        "id": ["Range_6"],
        "instanceType": ["Range"]
      }
    }
  ]
}"#,
    )
    .expect("write usdm data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_skips_core_000039_missing_svpresp_as_oracle_gap() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000039.json"),
        r#"{
  "Core": { "Id": "CORE-000039", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": {
    "Message": "VISITNUM for planned visit is not in TV.",
    "Output Variables": ["SVPRESP", "VISITNUM"]
  }
}"#,
    )
    .expect("write core 39 rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [1, 2]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [1, 99]
      }
    }
  ]
}"#,
    )
    .expect("write core 39 data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_ignores_missing_columns_for_non_applicable_scoped_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SCOPED-MISSING-COLUMN.json"),
        r#"{
  "Core": { "Id": "CORE-SCOPED-MISSING-COLUMN", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESTDTC",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "AESTDTC must be populated" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESTDTC": ["2020-01-01", ""]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSTDTC": ["2020-01-01"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "AE");
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_skips_missing_column_oracle_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000750.json"),
        r#"{
  "Core": { "Id": "CORE-000750", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "POOLID",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "USUBJID has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write rule");
    fs::write(
        rules_dir.join("CORE-000092.json"),
        r#"{
  "Core": { "Id": "CORE-000092", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECSTAT",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "ECSTAT has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write second rule");
    fs::write(
        rules_dir.join("CORE-000016.json"),
        r#"{
  "Core": { "Id": "CORE-000016", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "ECMOOD",
    "operator": "empty"
  },
  "Outcome": { "Message": "ECMOOD has oracle-specific missing-column semantics" }
}"#,
    )
    .expect("write empty missing-column rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": [""]
      }
    },
    {
      "filename": "ec.xpt",
      "domain": "EC",
      "records": {
        "USUBJID": ["S1"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 3);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Skipped));
    assert!(outcome
        .results
        .iter()
        .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
}

#[test]
fn run_validation_skips_core_000674_missing_placeholder_column_as_oracle_gap() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000674.json"),
        r#"{
  "Core": { "Id": "CORE-000674", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IQ"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--VALTRG", "operator": "matches_regex", "value": "^-?([1-9]\\d*|0)(\\.\\d+)?$" },
      { "name": "--VALMAX", "operator": "matches_regex", "value": "^.+$" },
      { "name": "--VALTRG", "operator": "greater_than", "value": "--VALMAX" }
    ]
  },
  "Outcome": { "Message": "--VALTRG must be <= --VALMAX" }
}"#,
    )
    .expect("write core 674 rule");

    let dataset_path = data_dir.join("iq.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "iq.csv",
      "domain": "IQ",
      "records": {
        "IQVALTRG": [1]
      }
    }
  ]
}"#,
    )
    .expect("write core 674 data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_requires_paths_before_loading() {
    let request = ValidateRequest {
        include_rules: Vec::new(),
        exclude_rules: Vec::new(),
        output_dir: None,
        rule_paths: Vec::new(),
        dataset_paths: Vec::new(),
        ..Default::default()
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
        r#""Operations": [{ "name": "future_operation" }],"#,
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
        ..Default::default()
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
fn run_validation_skips_unsupported_rules_before_loading_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::write(
        rules_dir.join("CORE-JSONATA-UNSUPPORTED.json"),
        r#"{
  "Core": { "Id": "CORE-JSONATA-UNSUPPORTED", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$.study.versions.studyDesigns.{\"id\": id}[id != null]",
  "Outcome": { "Message": "Unsupported JSONata" }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dir.path().join("missing-data")],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        output_dir: Some(output_dir.clone()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::UnsupportedOperator)
    );
    let report_csv = fs::read_to_string(output_dir.join("report.csv")).expect("read csv");
    assert!(report_csv.contains("CORE-JSONATA-UNSUPPORTED"));
    assert!(report_csv.contains("unsupported_operator"));
}

#[test]
fn run_validation_executes_open_rules_date_operators() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-DATE-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-DATE-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "STARTDTC",
    "operator": "date_greater_than",
    "value": "2024-01-01"
  },
  "Outcome": { "Message": "STARTDTC must be on or before 2024-01-01" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "AESEQ": [1],
        "STARTDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_core_000653_date_end_before_start() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000653.json"),
        r#"{
  "Core": { "Id": "CORE-000653", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENDTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "--ENDTC" }
    ]
  },
  "Outcome": {
    "Message": "--ENDTC must be greater than or equal to --DTC",
    "Output Variables": ["--ENDTC", "--DTC"]
  }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "DSSEQ": [1, 2],
        "DSDTC": ["2018-09-21", "2018-05-08T09:13"],
        "DSENDTC": ["2018-09-04", "2018-05-08T08:00"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000653");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 4);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].variables, vec!["DSENDTC"]);
    assert_eq!(outcome.results[0].errors[1].variables, vec!["DSDTC"]);
    assert_eq!(outcome.results[0].errors[2].row, Some(2));
    assert_eq!(outcome.results[0].errors[2].variables, vec!["DSENDTC"]);
    assert_eq!(outcome.results[0].errors[3].variables, vec!["DSDTC"]);
}

#[test]
fn run_validation_executes_core_000505_invalid_study_start_dates() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000505.json"),
        r#"{
  "Core": { "Id": "CORE-000505", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "TSPARMCD", "operator": "equal_to", "value": "SSTDTC" },
      { "name": "TSVAL", "operator": "invalid_date" }
    ]
  },
  "Outcome": { "Message": "TSVAL where TSPARMCD = SSTDTC is not in ISO 8601 format." }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "TSSEQ": [1, 2, 3, 4, 5],
        "TSPARMCD": ["SSTDTC", "SSTDTC", "SSTDTC", "SSTDTC", "SSTDTC"],
        "TSVAL": ["2003-12", "200", "2003-20", "2003-11-31", "2003-02-31"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000505");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 8);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].variables, vec!["TSPARMCD"]);
    assert_eq!(outcome.results[0].errors[1].variables, vec!["TSVAL"]);
    assert_eq!(outcome.results[0].errors[6].row, Some(5));
    assert_eq!(outcome.results[0].errors[6].variables, vec!["TSPARMCD"]);
    assert_eq!(outcome.results[0].errors[7].variables, vec!["TSVAL"]);
}

#[test]
fn run_validation_executes_core_000139_incomplete_reference_start_date() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
            rules_dir.join("CORE-000139.json"),
            r#"{
  "Core": { "Id": "CORE-000139", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--ENDTC", "operator": "is_incomplete_date" },
          { "name": "--ENDY", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "RFSTDTC", "operator": "is_incomplete_date" },
          { "name": "--ENDY", "operator": "non_empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "--ENDY is not null when either --ENDTC or DM.RFSTDTC do not contain complete values in their date portion",
    "Output Variables": ["--ENDTC", "RFSTDTC", "--ENDY"]
  }
}"#,
        )
        .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "EXSEQ": [1, 2],
        "EXENDTC": ["2012-11-30", "2012-12-01"],
        "EXENDY": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFSTDTC": ["2012-11"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000139");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 6);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].variables, vec!["EXENDTC"]);
    assert_eq!(outcome.results[0].errors[1].variables, vec!["RFSTDTC"]);
    assert_eq!(outcome.results[0].errors[2].variables, vec!["EXENDY"]);
    assert_eq!(outcome.results[0].errors[3].row, Some(2));
    assert_eq!(outcome.results[0].errors[3].variables, vec!["EXENDTC"]);
    assert_eq!(outcome.results[0].errors[4].variables, vec!["RFSTDTC"]);
    assert_eq!(outcome.results[0].errors[5].variables, vec!["EXENDY"]);
}

#[test]
fn run_validation_executes_core_000138_incomplete_start_dates_and_dm_dataset_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
            rules_dir.join("CORE-000138.json"),
            r#"{
  "Core": { "Id": "CORE-000138", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "--STDTC", "operator": "is_incomplete_date" },
          { "name": "--STDY", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "RFSTDTC", "operator": "is_incomplete_date" },
          { "name": "--STDY", "operator": "non_empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "--STDY is not null when either --STDTC or DM.RFSTDTC do not contain complete values in their date portion",
    "Output Variables": ["--STDY", "--STDTC", "RFSTDTC"]
  }
}"#,
        )
        .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["001", "002"],
        "AESEQ": [1, 1],
        "AESTDTC": ["2005-10", "2005-10-13T13:05"],
        "AESTDY": [1, 1]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["001", "002"],
        "RFSTDTC": ["2022-03-20", "2022-03"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let ae = outcome
        .results
        .iter()
        .find(|result| result.dataset == "AE")
        .expect("AE result");
    assert_eq!(ae.rule_id, "CORE-000138");
    assert_eq!(ae.execution_status, ExecutionStatus::Failed);
    assert_eq!(ae.error_count, 6);
    assert_eq!(ae.errors[0].row, Some(1));
    assert_eq!(ae.errors[0].variables, vec!["AESTDY"]);
    assert_eq!(ae.errors[1].variables, vec!["AESTDTC"]);
    assert_eq!(ae.errors[2].variables, vec!["RFSTDTC"]);
    assert_eq!(ae.errors[3].row, Some(2));
    assert_eq!(ae.errors[3].variables, vec!["AESTDY"]);
    assert_eq!(ae.errors[4].variables, vec!["AESTDTC"]);
    assert_eq!(ae.errors[5].variables, vec!["RFSTDTC"]);

    let dm = outcome
        .results
        .iter()
        .find(|result| result.dataset == "DM")
        .expect("DM result");
    assert_eq!(dm.execution_status, ExecutionStatus::Failed);
    assert_eq!(dm.error_count, 1);
    assert_eq!(dm.errors[0].row, None);
    assert!(dm.errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_core_000324_invalid_end_relative_timing() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
            rules_dir.join("CORE-000324.json"),
            r#"{
  "Core": { "Id": "CORE-000324", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_equal_to", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
        "filename": "cm.xpt",
        "domain": "CM",
        "records": {
          "USUBJID": ["SUBJ1"],
          "CMSEQ": [1],
          "CMENTPT": ["2013-05-20"],
          "CMENRTPT": ["AFTER"]
        }
      }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000324");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 3);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["CMDTC", "CMENRTPT", "CMENTPT"]);
}

#[test]
fn run_validation_executes_core_000460_invalid_trial_set_dates() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000460.json"),
        r#"{
  "Core": { "Id": "CORE-000460", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      {
        "any": [
          { "name": "TXPARMCD", "operator": "equal_to", "value": "DOSENDTC" },
          { "name": "TXPARMCD", "operator": "equal_to", "value": "DOSSTDTC" }
        ]
      },
      { "name": "TXVAL", "operator": "invalid_date" }
    ]
  },
  "Outcome": { "Message": "The value of TXVAL is not in ISO 8601 date/datetime format" }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "DOMAIN": ["TX"],
        "TXPARMCD": ["DOSSTDTC"],
        "TXVAL": ["2022-03-a"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000460");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["TXPARMCD", "TXVAL"]);
}

#[test]
fn run_validation_executes_core_000572_invalid_end_relative_timing_after_reference() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000572.json"),
            r#"{
  "Core": { "Id": "CORE-000572", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_less_than", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "AFTER", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'AFTER', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSEQ": [1],
        "CMDTC": ["2013-05-21"],
        "CMENTPT": ["2013-05-20"],
        "CMENRTPT": ["WRONG"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000572");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 3);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["CMDTC", "CMENRTPT", "CMENTPT"]);
}

#[test]
fn run_validation_executes_core_000572_cm_dataset_marker_when_dtc_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000572.json"),
            r#"{
  "Core": { "Id": "CORE-000572", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_less_than", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "AFTER", "UNKNOWN"] }
    ]
  },
  "Outcome": {
    "Message": "--ENRTPT is not in ('BEFORE', 'COINCIDENT', 'ONGOING', 'AFTER', 'UNKNOWN')",
    "Output Variables": ["--ENTPT", "--ENRTPT", "--DTC"]
  }
}"#,
        )
        .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "CMSEQ": [1],
        "CMENTPT": ["2013-05-20"],
        "CMENRTPT": ["WRONG"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    let marker = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("dataset marker");
    assert_eq!(marker.rule_id, "CORE-000572");
    assert_eq!(marker.dataset, "CM");
    assert_eq!(marker.error_count, 1);
    assert_eq!(marker.errors[0].row, None);
    assert!(marker.errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_core_000095_unplanned_element_description() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000095.json"),
        r#"{
  "Core": { "Id": "CORE-000095", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "SEUPDES", "operator": "non_empty" },
      { "name": "ETCD", "operator": "not_equal_to", "value": "UNPLAN" }
    ]
  },
  "Outcome": { "Message": "ETCD is not UNPLAN, when SEUPDES is not empty" }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "SESEQ": [1],
        "ETCD": ["TRTZ"],
        "SEUPDES": ["Unplanned treatment"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000095");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["SEUPDES".to_owned(), "ETCD".to_owned()]
    );
}

#[test]
fn run_validation_executes_core_000095_se_dataset_marker_when_seupdes_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000095.json"),
        r#"{
  "Core": { "Id": "CORE-000095", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "SEUPDES", "operator": "non_empty" },
      { "name": "ETCD", "operator": "not_equal_to", "value": "UNPLAN" }
    ]
  },
  "Outcome": { "Message": "ETCD is not UNPLAN, when SEUPDES is not empty" }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1"],
        "SESEQ": [1],
        "ETCD": ["TRTZ"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let marker = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("dataset marker");
    assert_eq!(marker.rule_id, "CORE-000095");
    assert_eq!(marker.dataset, "SE");
    assert_eq!(marker.error_count, 1);
    assert_eq!(marker.errors[0].row, None);
    assert!(marker.errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_core_000711_reference_start_after_end_dates() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000711.json"),
        r#"{
  "Core": { "Id": "CORE-000711", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "RFSTDTC", "operator": "non_empty" },
      { "name": "RFENDTC", "operator": "non_empty" },
      { "name": "RFSTDTC", "operator": "date_greater_than", "value": "RFENDTC" }
    ]
  },
  "Outcome": {
    "Message": "RFSTDTC falls after RFENDTC.",
    "Output Variables": ["RFSTDTC", "RFENDTC"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFSTDTC": ["2006-03"],
        "RFENDTC": ["2006-01-16"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000711");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["RFENDTC", "RFSTDTC"]);
}

#[test]
fn run_validation_executes_core_000714_treatment_start_after_end_dates() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000714.json"),
        r#"{
  "Core": { "Id": "CORE-000714", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "RFXSTDTC", "operator": "non_empty" },
      { "name": "RFXENDTC", "operator": "non_empty" },
      { "name": "RFXSTDTC", "operator": "date_greater_than", "value": "RFXENDTC" }
    ]
  },
  "Outcome": {
    "Message": "RFXSTDTC falls after RFXENDTC.",
    "Output Variables": ["RFXSTDTC", "RFXENDTC"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["SUBJ1"],
        "RFXSTDTC": ["2018-04-17"],
        "RFXENDTC": ["2018-04"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000714");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["RFXENDTC", "RFXSTDTC"]);
}

#[test]
fn run_validation_executes_core_000866_observation_start_after_end_dates() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000866.json"),
        r#"{
  "Core": { "Id": "CORE-000866", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENDTC", "operator": "exists" },
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--ENDTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "--ENDTC" }
    ]
  },
  "Outcome": {
    "Message": "--DTC falls after --ENDTC.",
    "Output Variables": ["--DTC", "--ENDTC"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBDTC": ["2018-11"],
        "LBENDTC": ["2018"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000866");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let mut variables = outcome.results[0]
        .errors
        .iter()
        .map(|issue| issue.variables.join("|"))
        .collect::<Vec<_>>();
    variables.sort();
    assert_eq!(variables, vec!["LBDTC", "LBENDTC"]);
}

#[test]
fn run_validation_executes_grouped_min_date_with_date_not_equal_to() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MIN-DATE-NOT-EQUAL.json"),
        r#"{
  "Core": { "Id": "CORE-MIN-DATE-NOT-EQUAL", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DS",
      "group": ["USUBJID"],
      "id": "$min_ds_dsstdtc",
      "name": "DSSTDTC",
      "operator": "min_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "DSTERM", "operator": "contains", "value": "INFORMED CONSENT" },
      { "name": "DSSTDTC", "operator": "date_not_equal_to", "value": "$min_ds_dsstdtc" }
    ]
  },
  "Outcome": { "Message": "DSSTDTC is not the earliest informed consent date" }
}"#,
    )
    .expect("write grouped min date rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "DSSEQ": [1, 2, 1],
        "DSTERM": ["INFORMED CONSENT", "INFORMED CONSENT", "INFORMED CONSENT"],
        "DSSTDTC": ["2020-01-03", "2020-01-01", "2020-02-01"]
      }
    }
  ]
}"#,
    )
    .expect("write grouped min date data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_target_is_not_sorted_by_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-SORT-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-SORT-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AESEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "AESTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "AESEQ is not chronological" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "AESEQ": [1, 3, 2],
        "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_executes_empty_within_except_last_row_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-END-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-END-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SEENDTC",
    "operator": "empty_within_except_last_row",
    "ordering": "SESTDTC",
    "value": "USUBJID"
  },
  "Outcome": { "Message": "SEENDTC is empty before the last row" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SESEQ": [1, 2, 3],
        "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"],
        "SEENDTC": ["2024-01-02", "", ""]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_not_present_on_multiple_rows_within_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-REL-OPERATOR.json"),
        r#"{
  "Core": { "Id": "CORE-REL-OPERATOR", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "not_present_on_multiple_rows_within",
    "within": "USUBJID"
  },
  "Outcome": { "Message": "RELID must appear on multiple rows" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_executes_is_not_unique_set_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-UNIQUE-SET.json"),
        r#"{
  "Core": { "Id": "CORE-UNIQUE-SET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be unique within subject" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1", "R2"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_executes_is_inconsistent_across_dataset_operator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-INCONSISTENT-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-INCONSISTENT-DATASET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_inconsistent_across_dataset",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID must be consistent within subject" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
        "RELID": ["R1", "R1", "R2", "R3"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 3);
}

#[test]
fn run_validation_reports_inconsistent_across_dataset_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000142.json"),
        r#"{
  "Core": { "Id": "CORE-000142", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "FTELTM",
    "operator": "is_inconsistent_across_dataset",
    "value": ["DOMAIN", "VISITNUM", "FTTPTREF", "FTTPTNUM"]
  },
  "Outcome": { "Message": "FTELTM has oracle-specific consistency semantics" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ft.xpt",
      "domain": "FT",
      "records": {
        "DOMAIN": ["FT", "FT"],
        "VISITNUM": [1, 1],
        "FTTPTREF": ["DOSE", "DOSE"],
        "FTTPTNUM": [1, 1],
        "FTELTM": ["PT30M", "PT03M"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_reports_unique_set_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000390.json"),
        r#"{
  "Core": { "Id": "CORE-000390", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "RELID",
    "operator": "is_not_unique_set",
    "value": ["USUBJID"]
  },
  "Outcome": { "Message": "RELID has oracle-specific uniqueness semantics" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "RELID": ["R1", "R1"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_reports_not_unique_relationship_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000184.json"),
        r#"{
  "Core": { "Id": "CORE-000184", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--BDSYCD",
    "operator": "is_not_unique_relationship",
    "value": "--BODSYS"
  },
  "Outcome": { "Message": "relationship has oracle-specific scope semantics" }
}"#,
    )
    .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "AEBDSYCD": ["10029205", "10029206"],
        "AEBODSYS": ["Nervous system disorders", "Nervous system disorders"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_executes_core_000651_missing_tptnum_as_dataset_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000651.json"),
        r#"{
  "Core": { "Id": "CORE-000651", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "--TPTNUM", "operator": "exists" },
    { "name": "--TPT", "operator": "exists" },
    { "name": "--TPTNUM", "operator": "non_empty" },
    { "name": "--TPT", "operator": "non_empty" },
    { "name": "--TPTNUM", "operator": "is_not_unique_relationship", "value": "--TPT" }
  ] },
  "Outcome": {
    "Message": "--TPT and --TPTNUM do not have a one-to-one relationship",
    "Output Variables": ["--TPT", "--TPTNUM"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBSEQ": [1, 2],
        "LBTPT": ["AM1", "AM2"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000651");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "LB");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert!(outcome.results[0].errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_core_000651_missing_pp_for_scat_tpt_relationship() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000651.json"),
        r#"{
  "Core": { "Id": "CORE-000651", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "--TPTNUM", "operator": "exists" },
    { "name": "--TPT", "operator": "exists" },
    { "name": "--TPTNUM", "operator": "non_empty" },
    { "name": "--TPT", "operator": "non_empty" },
    { "name": "--TPTNUM", "operator": "is_not_unique_relationship", "value": "--TPT" }
  ] },
  "Outcome": {
    "Message": "--TPT and --TPTNUM do not have a one-to-one relationship",
    "Output Variables": ["--TPT", "--TPTNUM"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "LBSEQ": [1, 2],
        "LBSCAT": ["SUB1", "SUB2"],
        "LBTPT": ["AM1", "AM2"],
        "LBTPTNUM": [1, 2]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        output_dir: None,
        ..Default::default()
    })
    .expect("run validation");

    let pp_result = outcome
        .results
        .iter()
        .find(|result| result.dataset == "PP")
        .expect("missing PP result");
    assert_eq!(pp_result.rule_id, "CORE-000651");
    assert_eq!(pp_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(pp_result.error_count, 1);
    assert_eq!(pp_result.errors[0].dataset, "PP");
    assert_eq!(pp_result.errors[0].row, None);
    assert!(pp_result.errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_dataset_presence_and_skips_known_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-DATASET-PRESENCE.json"),
        r#"{
  "Core": { "Id": "CORE-DATASET-PRESENCE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "exists" },
  "Outcome": { "Message": "presence semantics are not oracle-compatible yet" }
}"#,
    )
    .expect("write presence rule");
    fs::write(
        rules_dir.join("CORE-000015.json"),
        r#"{
  "Core": { "Id": "CORE-000015", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "exists" },
  "Outcome": { "Message": "known dataset presence gap" }
}"#,
    )
    .expect("write dataset presence gap rule");
    fs::write(
        rules_dir.join("CORE-COLUMN-REF.json"),
        r#"{
  "Core": { "Id": "CORE-COLUMN-REF", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "equal_to", "value": "AE--REF" },
  "Outcome": { "Message": "column-ref comparisons are not oracle-compatible yet" }
}"#,
    )
    .expect("write column-ref rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 3);
    let presence = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-DATASET-PRESENCE")
        .expect("presence result");
    assert_eq!(presence.execution_status, ExecutionStatus::Failed);
    assert_eq!(presence.error_count, 2);

    let gap = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-000015")
        .expect("gap result");
    assert_eq!(gap.execution_status, ExecutionStatus::Skipped);
    assert_eq!(gap.skipped_reason, Some(SkippedReason::OracleSemanticsGap));

    let column_ref = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-COLUMN-REF")
        .expect("column-ref result");
    assert_eq!(column_ref.execution_status, ExecutionStatus::Skipped);
}

#[test]
fn run_validation_executes_domain_presence_rule_against_loaded_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": { "DOMAIN": ["AE"] }
    },
    {
      "filename": "tt.csv",
      "domain": "TT",
      "records": { "DOMAIN": ["TT"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DOMAIN-PRESENCE.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-PRESENCE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Check": { "name": "TT", "operator": "exists" },
  "Outcome": { "Message": "TT dataset is present" }
}"#,
    )
    .expect("write domain presence rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "TT");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["TT"]);
}

#[test]
fn run_validation_executes_domain_presence_variable_exists_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pc.csv",
      "domain": "PC",
      "records": { "DOMAIN": ["PC"], "POOLID": ["POOL1"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DOMAIN-VARIABLE-EXISTS.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-VARIABLE-EXISTS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Operations": [{ "id": "$poolid_exists", "name": "POOLID", "operator": "variable_exists" }],
  "Check": {
    "all": [
      { "name": "$poolid_exists", "operator": "equal_to", "value": true },
      { "name": "POOLDEF", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "POOLID variable exists but POOLDEF dataset is missing",
    "Output Variables": ["$poolid_exists", "POOLDEF"]
  }
}"#,
    )
    .expect("write domain presence rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$poolid_exists", "POOLDEF"]
    );
}

#[test]
fn run_validation_executes_dataset_metadata_record_count_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.csv",
      "domain": "VS",
      "label": "Vital Signs",
      "records": { "DOMAIN": [] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DATASET-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-DATASET-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$record_count", "operator": "record_count" }],
  "Check": { "name": "$record_count", "operator": "equal_to", "value": 0 },
  "Outcome": {
    "Message": "Dataset may not be empty",
    "Output Variables": ["dataset_name", "dataset_label", "$record_count"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "VS");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["dataset_name", "dataset_label", "$record_count"]
    );
}

#[test]
fn run_validation_executes_dataset_metadata_domain_prefix_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ab.csv",
      "domain": "LB",
      "label": "Laboratory Test Results A",
      "records": { "DOMAIN": ["LB"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-DATASET-PREFIX.json"),
        r#"{
  "Core": { "Id": "CORE-DATASET-PREFIX", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Check": { "name": "dataset_name", "operator": "prefix_not_equal_to", "value": "DOMAIN" },
  "Outcome": {
    "Message": "Dataset name must begin with DOMAIN",
    "Output Variables": ["dataset_name"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "LB");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["dataset_name"]);
}

#[test]
fn run_validation_executes_dataset_metadata_dataset_names_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "qs36.csv",
      "domain": "QS",
      "label": "Questionnaires SF-36",
      "records": { "DOMAIN": ["QS"] }
    },
    {
      "filename": "ae.csv",
      "domain": "AE",
      "label": "Adverse Events",
      "records": { "DOMAIN": ["AE"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-DATASET-NAMES.json"),
            r#"{
  "Core": { "Id": "CORE-DATASET-NAMES", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$list_dataset_names", "operator": "dataset_names" }],
  "Check": {
    "all": [
      { "name": "dataset_name", "operator": "matches_regex", "value": "^[A-Z]{2}[A-Z0-9]{1,2}" },
      { "name": "dataset_name", "operator": "not_prefix_matches_regex", "prefix": 2, "value": "(AP|FA)" },
      { "name": "dataset_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$list_dataset_names" }
    ]
  },
  "Outcome": {
    "Message": "Split dataset parent domain is missing",
    "Output Variables": ["dataset_name", "$list_dataset_names"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("failed dataset metadata result");
    assert_eq!(failed.error_count, 1);
    assert_eq!(failed.errors[0].dataset, "QS");
    assert_eq!(
        failed.errors[0].variables,
        vec!["dataset_name", "$list_dataset_names"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_label_length_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "A label that is definitely longer than forty characters", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-VARIABLE-METADATA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Check": { "name": "variable_label", "operator": "longer_than", "value": 40 },
  "Outcome": {
    "Message": "Variable label is too long",
    "Output Variables": ["variable_label"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["variable_label"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_expected_variables_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
        r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$dataset_variables", "$expected_variables"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_required_variables_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-REQUIRED-VARIABLES.json"),
        r#"{
  "Core": { "Id": "CORE-REQUIRED-VARIABLES", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$required_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one required variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$required_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$dataset_variables", "$required_variables"]
    );
}

#[test]
fn run_validation_skips_core_000356_required_value_dataset_metadata_oracle_gap() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "AESEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": [""],
        "DOMAIN": ["AE"],
        "USUBJID": ["01"],
        "AESEQ": [1],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000356.json"),
        r#"{
  "Core": { "Id": "CORE-000356", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Dataset Metadata",
  "Operations": [
    { "id": "$required_variables", "operator": "required_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$required_variables", "operator": "exists" },
      { "name": "variable_name", "operator": "is_contained_by", "value": "$required_variables" },
      { "name": "variable_value", "operator": "empty" }
    ]
  },
  "Outcome": {
    "Message": "At least one Required variable has a null value",
    "Output Variables": ["variable_name", "variable_value"]
  }
}"#,
    )
    .expect("write value metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
    assert_eq!(outcome.results[0].error_count, 0);
    assert!(outcome.results[0].errors.is_empty());
}

#[test]
fn run_validation_passes_variable_metadata_expected_variables_when_all_are_present() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXDOSE", "label": "Dose", "type": "Num", "length": 8 },
        { "name": "EXDOSU", "label": "Dose Units", "type": "Char", "length": 20 },
        { "name": "EXDOSFRM", "label": "Dose Form", "type": "Char", "length": 20 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 },
        { "name": "EXENDTC", "label": "End Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXDOSE": ["10"],
        "EXDOSU": ["mg"],
        "EXDOSFRM": ["TABLET"],
        "EXSTDTC": ["2024-01-01"],
        "EXENDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-VARIABLE-EXPECTED.json"),
        r#"{
  "Core": { "Id": "CORE-000334", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$expected_variables", "operator": "expected_variables" },
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "not_contains_all", "value": ["$expected_variables"] }
    ]
  },
  "Outcome": {
    "Message": "At least one expected variable is missing from dataset",
    "Output Variables": ["$dataset_variables", "$expected_variables"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_executes_variable_metadata_timing_variables_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "EXSTDTC", "label": "Start Date", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["EX"],
        "EXSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000575.json"),
            r#"{
  "Core": { "Id": "CORE-000575", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$dataset_variables", "operator": "get_column_order_from_dataset" },
    { "id": "$timing_variables", "key_name": "role", "key_value": "Timing", "operator": "get_model_filtered_variables" }
  ],
  "Check": {
    "all": [
      { "name": "$dataset_variables", "operator": "shares_no_elements_with", "value": "$timing_variables" }
    ]
  },
  "Outcome": {
    "Message": "No timing variable is provided",
    "Output Variables": ["$dataset_variables", "$timing_variables"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .unwrap_or_else(|| panic!("AE timing failure: {:?}", outcome.results));
    assert_eq!(failed.dataset, "AE");
    assert_eq!(failed.error_count, 1);
    assert_eq!(failed.errors[0].row, None);
    assert_eq!(
        failed.errors[0].variables,
        vec!["$dataset_variables", "$timing_variables"]
    );
    assert!(
        !outcome
            .results
            .iter()
            .any(|result| result.dataset == "EX"
                && result.execution_status == ExecutionStatus::Failed)
    );
}

#[test]
fn run_validation_executes_variable_metadata_model_column_order_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AESTDYXX", "label": "Custom Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["AE"],
        "AETERM": ["HEADACHE"],
        "AESTDYXX": [1]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000550.json"),
            r#"{
  "Core": { "Id": "CORE-000550", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "Variables not listed in the Model List of Allowed Variables for Observation Class should be in SUPPQUAL.",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(outcome.results[0].errors[0].row, Some(4));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["variable_name"]
    );
}

#[test]
fn run_validation_executes_variable_metadata_library_column_order_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "class": "EVENTS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 }
      ],
      "records": {
        "DOMAIN": ["AE"],
        "STUDYID": ["CDISC-TEST"],
        "AETERM": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000852.json"),
            r#"{
  "Core": { "Id": "CORE-000852", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$column_order_from_library", "operator": "get_column_order_from_library" },
    { "id": "$column_order_from_dataset", "operator": "get_column_order_from_dataset" }
  ],
  "Check": {
    "all": [
      { "name": "$column_order_from_dataset", "operator": "is_not_ordered_subset_of", "value": "$column_order_from_library" }
    ]
  },
  "Outcome": {
    "Message": "Variables are not in the correct order.",
    "Output Variables": ["$column_order_from_dataset", "$column_order_from_library"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AE");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["$column_order_from_dataset", "$column_order_from_library"]
    );
}

#[test]
fn run_validation_executes_value_check_with_variable_metadata_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AETERM", "label": "Reported Term", "type": "Char", "length": 40 },
        { "name": "AECAT", "label": "Category", "type": "Char", "length": 40 },
        { "name": "AESTDY", "label": "Study Day", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST", "CDISC-TEST"],
        "AETERM": ["HEADACHE", " NAUSEA"],
        "AECAT": [".", "GENERAL"],
        "AESTDY": [1, 2]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    for (id, check) in [
        (
            "CORE-000867",
            r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "matches_regex", "value": "^\\s" }
    ] }"#,
        ),
        (
            "CORE-000890",
            r#"{ "all": [
      { "name": "variable_data_type", "operator": "equal_to", "value": "Char" },
      { "name": "variable_value", "operator": "non_empty" },
      { "name": "variable_value", "operator": "equal_to", "value": ".", "value_is_literal": true }
    ] }"#,
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{ "Include": ["ALL"] }}, "Classes": {{ "Include": ["ALL"] }} }},
  "Sensitivity": "Record",
  "Rule Type": "Value Check with Variable Metadata",
  "Check": {check},
  "Outcome": {{
    "Message": "Value metadata rule.",
    "Output Variables": ["variable_value", "variable_name"]
  }}
}}"#
            ),
        )
        .expect("write rule");
    }

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().any(|result| {
        result.rule_id == "CORE-000867"
            && result.errors[0].row == Some(2)
            && result.errors[0].variables == vec!["variable_value", "variable_name"]
    }));
    assert!(failed.iter().any(|result| {
        result.rule_id == "CORE-000890"
            && result.errors[0].row == Some(1)
            && result.errors[0].variables == vec!["variable_value", "variable_name"]
    }));
}

#[test]
fn run_validation_executes_selected_library_metadata_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AESDTH", "label": "Death", "type": "Char", "length": 1 }
      ],
      "records": { "STUDYID": ["S1"], "AESDTH": ["Y"] }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "XRACE", "label": "Race Extension", "type": "Char", "length": 20 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["DM"], "XRACE": ["BLUE"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Distinct Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Blabla", "type": "Char", "length": 8 },
        { "name": "VSORRESU", "label": "Original Units as Collected", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["VS"],
        "USUBJID": ["S1"],
        "VSSEQ": [1],
        "VSTESTCD": ["SYSBP"],
        "VSORRESU": ["mmHg"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
        rules_dir.join("CORE-000398.json"),
        r#"{
  "Core": { "Id": "CORE-000398", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "library_variable_label", "operator": "non_empty" },
    { "name": "variable_label", "operator": "not_equal_to", "value": "library_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable does not correspond to the label in the IG",
    "Output Variables": ["variable_name", "variable_label", "library_variable_label"]
  }
}"#,
    )
    .expect("write label rule");
    fs::write(
            rules_dir.join("CORE-000903.json"),
            r#"{
  "Core": { "Id": "CORE-000903", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "SE", "CO"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "exists" },
    { "name": "variable_name", "operator": "not_equal_to", "value": "library_variable_name" }
  ] },
  "Outcome": {
    "Message": "The variable is not allowed in this domain as it is not specified in the SENDIG for the specific domain",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write allowed-variable rule");
    fs::write(
        rules_dir.join("CORE-000507.json"),
        r#"{
  "Core": { "Id": "CORE-000507", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Define XML",
  "Check": { "all": [
    { "name": "variable_label", "operator": "not_equal_to", "value": "define_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable is incorrect",
    "Output Variables": ["define_variable_name", "define_variable_label", "variable_label"]
  }
}"#,
    )
    .expect("write define label rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 3);
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000398"
        && result.dataset == "AE"
        && result.errors[0].variables
            == vec!["variable_name", "variable_label", "library_variable_label"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000903"
        && result.dataset == "DM"
        && result.errors[0].variables == vec!["variable_name"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000507"
        && result.dataset == "VS"
        && result.error_count == 3));
}

#[test]
fn run_validation_executes_core_000929_domain_codelist_metadata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["FA"] }
    },
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["ZB"] }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
            rules_dir.join("CORE-000929.json"),
            r#"{
  "Core": { "Id": "CORE-000929", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$domain_is_custom", "operator": "domain_is_custom" },
    { "id": "$domain_lib_ccode", "operator": "codelist_terms", "codelists": ["DOMAIN"], "returntype": "code" }
  ],
  "Check": { "all": [
    { "name": "$domain_is_custom", "operator": "equal_to", "value": false },
    { "name": "define_variable_ccode", "operator": "equal_to", "value": "C66734" },
    { "name": "define_variable_codelist_coded_codes", "operator": "is_not_contained_by", "value": "$domain_lib_ccode" }
  ] },
  "Outcome": {
    "Message": "DOMAIN Code is not a published DOMAIN Code in CDISC Controlled Terminology.",
    "Output Variables": ["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
  }
}"#,
        )
        .expect("write domain codelist metadata rule");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.FA" Name="FA" Domain="FA">
    <ItemRef ItemOID="IT.FA.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.ZB" Name="ZB" Domain="ZB">
    <ItemRef ItemOID="IT.ZB.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemDef OID="IT.FA.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_FA"/>
  </ItemDef>
  <ItemDef OID="IT.ZB.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_ZB"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN_FA">
    <CodeListItem CodedValue="FA"><Alias Context="nci:ExtCodeID" Name="C00002"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
  <CodeList OID="CL.DOMAIN_ZB">
    <CodeListItem CodedValue="ZB"><Alias Context="nci:ExtCodeID" Name="C00003"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(data_dir.join(".env"), "VERSION=3-3\n").expect("write env");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000929");
    assert_eq!(failed[0].dataset, "FA");
    assert_eq!(failed[0].error_count, 1);
    assert_eq!(
        failed[0].errors[0].variables,
        vec!["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
    );
}

#[test]
fn run_validation_executes_core_000494_define_role_metadata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 }
      ],
      "records": { "DOMAIN": ["VS"], "VSTESTCD": ["SYSBP"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.VS" Name="VS" Domain="VS">
    <ItemRef ItemOID="IT.VS.DOMAIN" OrderNumber="2" Role="WRONG: Domain Identifier"/>
    <ItemRef ItemOID="IT.VS.VSTESTCD" OrderNumber="5" Role="Topic"/>
  </ItemGroupDef>
  <ItemDef OID="IT.VS.DOMAIN" Name="DOMAIN">
    <Description><TranslatedText>Domain Abbreviation</TranslatedText></Description>
  </ItemDef>
  <ItemDef OID="IT.VS.VSTESTCD" Name="VSTESTCD">
    <Description><TranslatedText>Vital Signs Test Short Name</TranslatedText></Description>
  </ItemDef>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(
            rules_dir.join("CORE-000494.json"),
            r#"{
  "Core": { "Id": "CORE-000494", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "define_variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "define_variable_role", "operator": "not_equal_to", "value": "library_variable_role" }
  ] },
  "Outcome": {
    "Message": "The Role of the variable in the define.xml does not correspond to the Role given by the Implementation Guide",
    "Output Variables": [
      "define_variable_label",
      "define_variable_name",
      "define_variable_role",
      "library_variable_name",
      "library_variable_role"
    ]
  }
}"#,
        )
        .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000494");
    assert_eq!(failed[0].dataset, "VS");
    assert_eq!(failed[0].errors[0].row, Some(1));
    assert_eq!(
        failed[0].errors[0].variables,
        vec![
            "define_variable_label",
            "define_variable_name",
            "define_variable_role",
            "library_variable_name",
            "library_variable_role"
        ]
    );
}

#[test]
fn run_validation_executes_core_000595_missing_casno_oracle_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000595.json"),
            r#"{
  "Core": { "Id": "CORE-000595", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IN"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "any": [
    { "all": [
      { "name": "UNII", "operator": "empty" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "not_exists" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "CASNO", "operator": "not_exists" },
      { "name": "UNII", "operator": "empty" }
    ] }
  ] },
  "Outcome": {
    "Message": "At least one of the UNII and CASNO variables should be present and populated for each ingredient if available.",
    "Output Variables": ["UNII", "CASNO"]
  }
}"#,
        )
        .expect("write rule");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "in.xpt",
      "domain": "IN",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 12 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "UNII", "label": "Unique Ingredient Identifier", "type": "Char", "length": 50 }
      ],
      "records": {
        "STUDYID": ["TOB07"],
        "DOMAIN": ["IN"],
        "UNII": ["UNI2"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert!(outcome.results[0].errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_send_variable_metadata_model_column_order_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 },
        { "name": "VSTEST", "label": "Vital Signs Test Name", "type": "Char", "length": 40 },
        { "name": "VSNONSEN", "label": "Vital Signs Nonsense", "type": "Char", "length": 40 },
        { "name": "VSORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "VSNOTDY", "label": "Non Study Day of Vital Signs", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["VS"],
        "USUBJID": ["01"],
        "VSSEQ": [1],
        "VSTESTCD": ["WEIGHT"],
        "VSTEST": ["Weight"],
        "VSNONSEN": ["bad"],
        "VSORRES": ["80"],
        "VSNOTDY": [1]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000902.json"),
        r#"{
  "Core": { "Id": "CORE-000902", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "The variable is not an allowed variable for the underlying Observation Class",
    "Output Variables": ["variable_name"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let rows = outcome.results[0]
        .errors
        .iter()
        .map(|error| error.row)
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![Some(7), Some(9)]);
}

#[test]
fn run_validation_executes_custom_domain_variable_prefix_metadata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "ZBSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "LBORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "ZBORRESU", "label": "Original Units", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["ZB"],
        "USUBJID": ["01"],
        "ZBSEQ": [1],
        "LBORRES": ["80"],
        "ZBORRESU": ["kg"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000376.json"),
            r#"{
  "Core": { "Id": "CORE-000376", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$domain_list", "name": "DOMAIN", "operator": "distinct" },
    { "id": "$domain_is_custom", "operator": "domain_is_custom" }
  ],
  "Check": {
    "all": [
      { "name": "$domain_is_custom", "operator": "equal_to", "value": true },
      { "name": "variable_name", "operator": "is_not_contained_by", "value": ["STUDYID", "DOMAIN", "USUBJID"] },
      { "name": "variable_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$domain_list" }
    ]
  },
  "Outcome": {
    "Message": "First 2 characters of prefixed variable within custom domain do not match the DOMAIN value.",
    "Output Variables": ["$domain_list", "variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "ZB");
    assert_eq!(outcome.results[0].errors[0].row, Some(5));
}

#[test]
fn run_validation_skips_wildcard_target_rules_before_engine_execution() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-WILDCARD-TARGET.json"),
        r#"{
  "Core": { "Id": "CORE-WILDCARD-TARGET", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "--TESTCD", "operator": "not_matches_regex", "value": "^[A-Z]+$" },
  "Outcome": { "Message": "wildcard target expansion is not oracle-compatible yet" }
}"#,
    )
    .expect("write wildcard rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_reports_empty_non_empty_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000117.json"),
        r#"{
  "Core": { "Id": "CORE-000117", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "DTHDTC", "operator": "non_empty" },
      { "name": "DTHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "DTHDTC is populated but DTHFL is not Y" }
}"#,
    )
    .expect("write quarantined empty rule");

    let dataset_path = data_dir.join("dm-fail.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": ["2024-01-01"],
        "DTHFL": [""]
      }
    }
  ]
}"#,
    )
    .expect("write fail dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);

    let pass_path = data_dir.join("dm-pass.json");
    fs::write(
        &pass_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": [""],
        "DTHFL": [""]
      }
    }
  ]
}"#,
    )
    .expect("write pass dataset");

    let pass_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![pass_path],
        ..Default::default()
    })
    .expect("run pass validation");

    assert_eq!(pass_outcome.results.len(), 1);
    assert_eq!(
        pass_outcome.results[0].execution_status,
        ExecutionStatus::Passed
    );
    assert_eq!(pass_outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_passes_safe_empty_non_empty_oracle_gap_case() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000007.json"),
        r#"{
  "Core": { "Id": "CORE-000007", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "DTHDTC", "operator": "non_empty" },
      { "name": "DTHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "DTHDTC is populated but DTHFL is not Y" }
}"#,
    )
    .expect("write empty gap rule");
    let dataset_path = data_dir.join("dm.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.csv",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DTHDTC": [""],
        "DTHFL": [""]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_executes_core_000583_trial_summary_value_exclusive_or() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000583.json"),
        r#"{
  "Core": { "Id": "CORE-000583", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "all": [
          { "name": "TSVAL", "operator": "non_empty" },
          { "name": "TSVALNF", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "TSVAL", "operator": "empty" },
          { "name": "TSVALNF", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Either both TSVALNF and TSVAL are populated, or both are empty."
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("ts.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ts.csv",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "TSPARMCD": ["A", "B", "C"],
        "TSVAL": ["VALUE", "", "VALUE"],
        "TSVALNF": ["NF", "", ""]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000583");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000466_missing_uschfl_as_null() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000466.json"),
        r#"{
  "Core": { "Id": "CORE-000466", "Status": "Published" },
  "Scope": {
    "Domains": { "Include": ["ALL"] },
    "Classes": { "Include": ["FINDINGS", "EVENTS", "INTERVENTIONS"] }
  },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--USCHFL", "operator": "non_empty" },
      { "name": "--USCHFL", "operator": "not_equal_to", "value": "Y" }
    ]
  },
  "Outcome": { "Message": "--USCHFL must be either Y or null" }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "LBSEQ": [1, 2],
        "LBUSCHFL": ["maybe", "Y"]
      }
    },
    {
      "filename": "pp.csv",
      "domain": "PP",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["PP"],
        "USUBJID": ["SUBJ1"],
        "PPSEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert_eq!(outcome.results[0].dataset, "LB");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[1].dataset, "PP");
    assert_eq!(outcome.results[1].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[1].skipped_reason, None);

    let positive_path = data_dir.join("positive.json");
    fs::write(
        &positive_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBUSCHFL": ["Y"]
      }
    },
    {
      "filename": "pp.csv",
      "domain": "PP",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["PP"],
        "USUBJID": ["SUBJ1"],
        "PPSEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write positive dataset");

    let positive = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![positive_path],
        ..Default::default()
    })
    .expect("run positive validation");

    let marker = positive
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("pp marker");
    assert_eq!(marker.rule_id, "CORE-000466");
    assert_eq!(marker.dataset, "PP");
    assert_eq!(marker.error_count, 1);
    assert_eq!(marker.errors[0].row, None);
    assert!(marker.errors[0].variables.is_empty());
}

#[test]
fn run_validation_reports_date_operator_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000370.json"),
        r#"{
  "Core": { "Id": "CORE-000370", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DVSTDTC", "operator": "date_less_than", "value": "RFICDTC" },
  "Outcome": { "Message": "DVSTDTC date comparison semantics are not oracle-compatible yet" }
}"#,
    )
    .expect("write date gap rule");
    let dataset_path = data_dir.join("dv.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dv.csv",
      "domain": "DV",
      "records": {
        "USUBJID": ["SUBJ1"],
        "DVSEQ": [1],
        "DVSTDTC": ["2024-01-01"],
        "RFICDTC": ["2024-01-02"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_passes_safe_date_oracle_gap_case() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000324.json"),
            r#"{
  "Core": { "Id": "CORE-000324", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MH"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENTPT", "operator": "non_empty" },
      { "name": "--ENTPT", "operator": "is_complete_date" },
      { "name": "--DTC", "operator": "exists" },
      { "name": "--ENTPT", "operator": "date_equal_to", "value": "--DTC" },
      { "name": "--ENRTPT", "operator": "is_not_contained_by", "value": ["BEFORE", "COINCIDENT", "ONGOING", "UNKNOWN"] }
    ]
  },
  "Outcome": { "Message": "--ENRTPT has invalid date-relative semantics" }
}"#,
        )
        .expect("write date gap rule");
    let dataset_path = data_dir.join("mh.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "mh.csv",
      "domain": "MH",
      "records": {
        "USUBJID": ["S1"],
        "MHSEQ": [1],
        "MHDTC": ["2013-05-20"],
        "MHENTPT": ["2013-05-20"],
        "MHENRTPT": ["BEFORE"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_reports_sort_operator_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000535.json"),
        r#"{
  "Core": { "Id": "CORE-000535", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SM"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "SMSEQ",
    "operator": "target_is_not_sorted_by",
    "within": "USUBJID",
    "value": [
      { "name": "SMSTDTC", "sort_order": "asc", "null_position": "last" }
    ]
  },
  "Outcome": { "Message": "SMSEQ partial-date sort semantics are not oracle-compatible yet" }
}"#,
    )
    .expect("write sort gap rule");
    let dataset_path = data_dir.join("sm.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "sm.csv",
      "domain": "SM",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1"],
        "SMSEQ": [1, 3, 2],
        "SMSTDTC": ["2024-01-01", "2024-01-02", "2024-01-03"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_skips_etcd_length_rules_for_se_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ETCD-SE-LENGTH.json"),
        r#"{
  "Core": { "Id": "CORE-ETCD-SE-LENGTH", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SE"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "ETCD", "operator": "longer_than", "value": 8 },
  "Outcome": { "Message": "SE ETCD length semantics are not oracle-compatible yet" }
}"#,
    )
    .expect("write ETCD rule");

    let dataset_path = data_dir.join("se.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "se.csv",
      "domain": "SE",
      "records": {
        "ETCD": ["SCREENING"]
      }
    }
  ]
}"#,
    )
    .expect("write SE data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_skips_cross_domain_armcd_txval_length_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-ARMCD-TXVAL-LENGTH.json"),
            r#"{
  "Core": { "Id": "CORE-ARMCD-TXVAL-LENGTH", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "TA", "TX"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      { "name": "ARMCD", "operator": "longer_than", "value": 20 },
      {
        "all": [
          { "name": "TXPARMCD", "operator": "equal_to", "value": "ARMCD" },
          { "name": "TXVAL", "operator": "longer_than", "value": 20 }
        ]
      }
    ]
  },
  "Outcome": { "Message": "cross-domain ARMCD/TXVAL length semantics are not oracle-compatible yet" }
}"#,
        )
        .expect("write ARMCD/TXVAL rule");

    let dataset_path = data_dir.join("ta.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ta.csv",
      "domain": "TA",
      "records": {
        "ARMCD": ["THIS_ARM_CODE_IS_TOO_LONG"]
      }
    }
  ]
}"#,
    )
    .expect("write TA data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_filters_execution_datasets_by_entity_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-SCOPE.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-SCOPE", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "instanceType",
    "operator": "equal_to",
    "value": "StudyEpoch",
    "value_is_literal": true
  },
  "Outcome": { "Message": "StudyEpoch rows are checked once" }
}"#,
    )
    .expect("write entity scope rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "id": ["StudyEpoch_1"],
        "instanceType": ["StudyEpoch"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].dataset, "StudyEpoch");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_skips_entity_scope_column_ref_comparators() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-COLUMN-REF.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-COLUMN-REF", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "nextId",
    "operator": "not_equal_to",
    "value": "parent_id"
  },
  "Outcome": { "Message": "Entity relationship comparisons need entity semantics" }
}"#,
    )
    .expect("write entity column-ref rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "nextId": ["StudyEpoch_2"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_executes_entity_scope_missing_column_ref_literal_fallback() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-LITERAL-FALLBACK.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-LITERAL-FALLBACK", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "rel_type",
    "operator": "equal_to",
    "value": "definition"
  },
  "Outcome": { "Message": "definition activities are checked" }
}"#,
    )
    .expect("write entity literal fallback rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "id": ["Activity_1", "Activity_2"],
        "rel_type": ["definition", "instance"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000857_entity_codelist_column_refs() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000857.json"),
            r#"{
  "Core": { "Id": "CORE-000857", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Code",
      "Keys": [
        { "Left": "instanceType", "Right": "parent_entity" },
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Operations": [
    {
      "id": "$codelist_code",
      "operator": "map",
      "map": [{ "parent_rel.Code": "plannedSex", "output": "C66732" }]
    },
    { "id": "$valid_versions", "operator": "valid_codelist_dates" },
    {
      "id": "$codelist_extensible",
      "operator": "codelist_extensible",
      "codelist_code": "$codelist_code"
    },
    {
      "id": "$value_for_code",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "value",
      "term_code": "code"
    },
    {
      "id": "$pref_term_for_code",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "pref_term",
      "term_code": "code"
    },
    {
      "id": "$code_for_decode_pref_term",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "code",
      "term_pref_term": "decode"
    },
    {
      "id": "$code_for_decode_value",
      "operator": "codelist_terms",
      "codelist_code": "$codelist_code",
      "returntype": "code",
      "term_value": "decode"
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "plannedSex", "operator": "equal_to", "value": true },
      { "name": "parent_rel.Code", "operator": "equal_to", "value": "plannedSex", "value_is_literal": true },
      {
        "not": {
          "all": [
            { "name": "codeSystem", "operator": "equal_to", "value": "http://www.cdisc.org" },
            { "name": "codeSystemVersion", "operator": "is_contained_by", "value": "$valid_versions" },
            {
              "any": [
                {
                  "all": [
                    { "name": "$pref_term_for_code", "operator": "non_empty" },
                    { "name": "$value_for_code", "operator": "non_empty" },
                    {
                      "any": [
                        { "name": "$code_for_decode_pref_term", "operator": "non_empty" },
                        { "name": "$code_for_decode_value", "operator": "non_empty" }
                      ]
                    },
                    {
                      "any": [
                        { "name": "code", "operator": "equal_to", "value": "$code_for_decode_pref_term" },
                        { "name": "code", "operator": "equal_to", "value": "$code_for_decode_value" }
                      ]
                    },
                    {
                      "any": [
                        { "name": "decode", "operator": "equal_to", "value": "$pref_term_for_code" },
                        { "name": "decode", "operator": "equal_to", "value": "$value_for_code" }
                      ]
                    }
                  ]
                },
                {
                  "all": [
                    { "name": "$codelist_extensible", "operator": "equal_to", "value": true },
                    { "name": "$code_for_decode_pref_term", "operator": "empty" },
                    { "name": "$code_for_decode_value", "operator": "empty" },
                    { "name": "$pref_term_for_code", "operator": "empty" },
                    { "name": "$value_for_code", "operator": "empty" }
                  ]
                }
              ]
            }
          ]
        }
      }
    ]
  },
  "Outcome": {
    "Message": "planned sex codelist mismatch",
    "Output Variables": ["code", "decode", "$value_for_code", "$pref_term_for_code"]
  }
}"#,
        )
        .expect("write CORE-000857 rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "id": ["StudyDesignPopulation_1"],
        "instanceType": ["StudyDesignPopulation"],
        "rel_type": ["definition"],
        "plannedSex": [true]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["StudyDesignPopulation_1"],
        "parent_rel.Code": ["plannedSex"],
        "rel_type": ["definition"],
        "codeSystem": ["http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15"],
        "code": ["C16576"],
        "decode": ["Wrong"],
        "id": ["Code_1"],
        "name": ["Wrong code"]
      }
    }
  ]
}"#,
    )
    .expect("write entity codelist data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn static_codelist_resolves_ddf_organization_type_terms() {
    assert!(valid_codelist_dates().contains(&"2025-09-26"));

    let codelist = static_codelist("C188724").expect("organization type codelist");
    assert!(codelist.extensible);

    let sponsor = codelist
        .find_by_code("C70793")
        .expect("clinical study sponsor");
    assert_eq!(sponsor.value, "Study Sponsor");
    assert_eq!(sponsor.pref_term, "Clinical Study Sponsor");

    let registry = codelist
        .find_by_pref_term("Study Registry")
        .expect("study registry");
    assert_eq!(registry.code, "C93453");
    assert_eq!(registry.value, "Clinical Study Registry");

    let drug_company = codelist
        .find_by_value("Drug Company")
        .expect("drug company submission value");
    assert_eq!(drug_company.code, "C54149");
    assert_eq!(drug_company.pref_term, "Pharmaceutical Company");
}

#[test]
fn run_validation_reports_entity_literal_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000820.json"),
        r#"{
  "Core": { "Id": "CORE-000820", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "type",
    "operator": "equal_to",
    "value": "anchor"
  },
  "Outcome": { "Message": "entity literal oracle semantics are not supported" }
}"#,
    )
    .expect("write entity oracle gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "id": ["Timing_1"],
        "type": ["anchor"]
      }
    }
  ]
}"#,
    )
    .expect("write entity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
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
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-JSONATA");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["DOMAIN".to_owned()]
    );
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
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_executes_join_operation_with_different_key_names() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-JOIN-LOOKUP.json"),
        r#"{
  "Core": { "Id": "CORE-JOIN-LOOKUP", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "type": "lookup",
      "leftDataset": "AE",
      "rightDataset": "LOOKUP",
      "leftKeys": ["USUBJID"],
      "rightKeys": ["SUBJECT"],
      "prefix": "LOOKUP."
    }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
    )
    .expect("write lookup rule");

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
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "SUBJECT": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
    )
    .expect("write lookup data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["LOOKUP.FLAG".to_owned()]
    );
}

#[test]
fn run_validation_join_operation_uses_current_pipeline_left_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-FILTER-JOIN.json"),
        r#"{
  "Core": { "Id": "CORE-FILTER-JOIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "filter",
      "dataset": "AE",
      "where": {
        "name": "AESEQ",
        "operator": "greater_than",
        "value": 1
      }
    },
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
  "Outcome": { "Message": "Filtered-out supplemental values must not reappear" }
}"#,
    )
    .expect("write filter join rule");

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
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S1", "S2"],
        "QVAL": ["BAD", "OK"]
      }
    }
  ]
}"#,
    )
    .expect("write filter join data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_executes_match_datasets_without_explicit_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MATCH-DATASETS.json"),
        r#"{
  "Core": { "Id": "CORE-MATCH-DATASETS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "domain": "AE" },
    { "domain": "LOOKUP", "prefix": "LOOKUP." }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
    )
    .expect("write match datasets rule");

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
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
    )
    .expect("write match datasets data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["LOOKUP.FLAG".to_owned()]
    );
}

#[test]
fn run_validation_joins_single_match_dataset_to_scoped_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-SINGLE-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-SINGLE-MATCH-DATASET", "Status": "Published" },
  "Scope": {
    "Domains": { "Include": ["AE"] },
    "Classes": { "Include": ["EVENTS"] }
  },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SUPPAE", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "QNAM",
    "operator": "equal_to",
    "value": "AESOSP"
  },
  "Outcome": { "Message": "AESOSP supplemental qualifier must be reviewed" }
}"#,
    )
    .expect("write match dataset rule");

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
        "QNAM": ["AESOSP"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_joins_match_dataset_with_left_right_keys() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-LEFT-RIGHT-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-LEFT-RIGHT-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "LOOKUP",
      "Keys": [
        { "Left": "USUBJID", "Right": "SUBJECT" },
        "DOMAIN"
      ]
    }
  ],
  "Check": {
    "name": "FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
    )
    .expect("write match dataset rule");

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
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "SUBJECT": ["S2"],
        "DOMAIN": ["AE"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["FLAG".to_owned()]
    );
}

#[test]
fn run_validation_joins_usdm_match_dataset_before_unique_set() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-USDM-MATCH-DATASET.json"),
            r#"{
  "Core": { "Id": "CORE-USDM-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Code"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Encounter",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "Code" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Encounter" },
      { "name": "parent_rel", "operator": "equal_to", "value": "environmentalSetting", "value_is_literal": true },
      {
        "name": "code",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_rel", "parent_id", "codeSystem", "codeSystemVersion"]
      }
    ]
  },
  "Outcome": { "Message": "Duplicate environmental setting" }
}"#,
        )
        .expect("write USDM match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Encounter.csv",
      "domain": "Encounter",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["encounters"],
        "rel_type": ["definition"],
        "id": ["Encounter_1"],
        "name": ["E1"],
        "instanceType": ["Encounter"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["Encounter", "Encounter"],
        "parent_id": ["Encounter_1", "Encounter_1"],
        "parent_rel": ["environmentalSetting", "environmentalSetting"],
        "rel_type": ["definition", "definition"],
        "id": ["Code_84", "Code_85"],
        "code": ["C51282", "C51282"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "decode": ["Clinic", "Hospital"],
        "instanceType": ["Code", "Code"]
      }
    }
  ]
}"#,
    )
    .expect("write USDM match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_scoped_entity_through_multiple_match_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MULTI-USDM-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-MULTI-USDM-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "GovernanceDate",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    },
    {
      "Name": "GeographicScope",
      "Keys": [
        { "Left": "id.GovernanceDate", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "name": "id",
    "operator": "is_not_unique_set",
    "value": ["type.code", "type.code.GeographicScope"]
  },
  "Outcome": { "Message": "Governance dates must be unique by type and geographic scope" }
}"#,
    )
    .expect("write multi-match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyVersion.csv",
      "domain": "StudyVersion",
      "records": {
        "id": ["StudyVersion_1"],
        "rel_type": ["definition"],
        "instanceType": ["StudyVersion"]
      }
    },
    {
      "filename": "GovernanceDate.csv",
      "domain": "GovernanceDate",
      "records": {
        "parent_id": ["StudyVersion_1", "StudyVersion_1"],
        "rel_type": ["definition", "definition"],
        "id": ["GovernanceDate_1", "GovernanceDate_2"],
        "type.code": ["effective", "effective"],
        "instanceType": ["GovernanceDate", "GovernanceDate"]
      }
    },
    {
      "filename": "GeographicScope.csv",
      "domain": "GeographicScope",
      "records": {
        "parent_id": ["GovernanceDate_1", "GovernanceDate_2"],
        "rel_type": ["definition", "definition"],
        "id": ["GeographicScope_1", "GeographicScope_2"],
        "type.code": ["global", "global"],
        "instanceType": ["GeographicScope", "GeographicScope"]
      }
    }
  ]
}"#,
    )
    .expect("write multi-match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_treats_missing_left_match_dataset_as_no_reference_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MISSING-LEFT-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-MISSING-LEFT-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyEpoch"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "epochId" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyEpoch" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The epoch is not referenced by any scheduled activity instances.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name"]
  }
}"#,
    )
    .expect("write missing match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["epochs", "epochs"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyEpoch_1", "StudyEpoch_2"],
        "name": ["Screening", "Treatment"],
        "instanceType": ["StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
    )
    .expect("write missing match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_treats_missing_yaml_left_match_dataset_as_no_reference_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000816.yml"),
        r#"Check:
  all:
    - name: instanceType
      operator: equal_to
      value: 'StudyEpoch'
    - name: rel_type
      operator: equal_to
      value: 'definition'
    - any:
        - name: epochId
          operator: not_exists
        - name: epochId
          operator: empty
Core:
  Id: 'CORE-000816'
  Status: Published
Match Datasets:
  - Join Type: left
    Keys:
      - Left: id
        Right: epochId
      - rel_type
    Name: ScheduledActivityInstance
Outcome:
  Message: 'The epoch is not referenced by any scheduled activity instances.'
  Output Variables:
    - parent_entity
    - parent_id
    - parent_rel
    - id
    - name
Rule Type: Record Data
Scope:
  Entities:
    Include:
      - 'StudyEpoch'
Sensitivity: Record
"#,
    )
    .expect("write missing yaml match dataset rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Dataset Name,Label\nStudyEpoch,StudyEpoch,Study Epoch\n",
    )
    .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nStudyEpoch,parent_entity,Parent Entity Name,String,[1]\nStudyEpoch,parent_id,Parent Entity Id,String,[1]\nStudyEpoch,parent_rel,Name of Relationship from Parent Entity,String,[1]\nStudyEpoch,rel_type,Type of Relationship,String,[1]\nStudyEpoch,id,Identifier,String,[1]\nStudyEpoch,name,Name,String,[1]\nStudyEpoch,instanceType,Instance Type,String,[1]\nStudyEpoch,type,Study Epoch Type,Boolean,Code[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("StudyEpoch.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType,type\nStudyDesign,StudyDesign_1,epochs,definition,StudyEpoch_1,Screening,StudyEpoch,True\nStudyDesign,StudyDesign_1,epochs,definition,StudyEpoch_2,Treatment,StudyEpoch,True\n",
        )
        .expect("write study epoch csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_schedule_timeline_for_activity_epoch_presence() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["SCREEN1", "AE"],
        "epochId": ["", ""],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimeline.csv",
      "domain": "ScheduleTimeline",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "mainTimeline": [true, false],
        "instanceType": ["ScheduleTimeline", "ScheduleTimeline"]
      }
    }
  ]
}"#,
    )
    .expect("write schedule timeline match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_schedule_timeline_from_open_rules_csv() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000870.json"),
            r#"{
  "Core": { "Id": "CORE-000870", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimeline",
      "Keys": [
        { "Left": "parent_entity", "Right": "instanceType" },
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "mainTimeline", "operator": "equal_to", "value": true },
      {
        "any": [
          { "name": "epochId", "operator": "not_exists" },
          { "name": "epochId", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance is in the main timeline but does not refer to an epoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "mainTimeline", "id", "name", "epochId"]
  }
}"#,
        )
        .expect("write schedule timeline match rule");

    fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\nScheduleTimeline,ScheduleTimeline,Schedule Timeline\n",
        )
        .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Identifier,String,[1]\nScheduledActivityInstance,name,Name,String,[1]\nScheduledActivityInstance,epochId,Epoch Identifier,String,StudyEpoch[0..1].id[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\nScheduleTimeline,parent_entity,Parent Entity Name,String,[1]\nScheduleTimeline,parent_id,Parent Entity Id,String,[1]\nScheduleTimeline,rel_type,Type of Relationship,String,[1]\nScheduleTimeline,id,Identifier,String,[1]\nScheduleTimeline,mainTimeline,Main Timeline Indicator,Boolean,[1]\nScheduleTimeline,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,epochId,instanceType\nScheduleTimeline,ScheduleTimeline_1,instances,definition,ScheduledActivityInstance_1,SCREEN1,,ScheduledActivityInstance\nScheduleTimeline,ScheduleTimeline_2,instances,definition,ScheduledActivityInstance_2,AE,,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");
    fs::write(
            data_dir.join("ScheduleTimeline.csv"),
            "parent_entity,parent_id,rel_type,id,mainTimeline,instanceType\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_1,True,ScheduleTimeline\nStudyDesign,StudyDesign_1,definition,ScheduleTimeline_2,False,ScheduleTimeline\n",
        )
        .expect("write schedule timeline csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_suffixes_referenced_match_columns_without_left_conflict() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Planned age must be specified either in the study population or in all cohorts." }
}"#,
        )
        .expect("write suffix match column rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population without age column"],
        "instanceType": ["StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation"],
        "parent_id": ["Population_1"],
        "parent_rel": ["cohorts"],
        "rel_type": ["definition"],
        "id": ["Cohort_1"],
        "name": ["Cohort age"],
        "plannedAge": [true],
        "instanceType": ["StudyCohort"]
      }
    }
  ]
}"#,
    )
    .expect("write suffix match column data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_treats_missing_left_study_cohort_as_null_join_columns() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000815.json"),
        r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["id.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
    )
    .expect("write missing cohort rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["population"],
        "rel_type": ["definition"],
        "id": ["Population_1"],
        "name": ["Population age"],
        "plannedAge": [true],
        "instanceType": ["StudyDesignPopulation"]
      }
    }
  ]
}"#,
    )
    .expect("write missing cohort data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_left_joins_study_cohort_for_population_planned_sex_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000875.json"),
            r#"{
  "Core": { "Id": "CORE-000875", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedSex", "operator": "not_exists" },
                  { "name": "plannedSex", "operator": "empty" },
                  { "name": "plannedSex", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedSex.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedSex.StudyCohort", "operator": "empty" },
                  { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedSex", "operator": "equal_to", "value": true },
              { "name": "plannedSex.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned sex must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedSex", "id.StudyCohort", "name.StudyCohort", "plannedSex.StudyCohort"]
  }
}"#,
        )
        .expect("write planned sex rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedSex": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort sex", "No cohort sex"],
        "plannedSex": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned sex data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_study_cohort_for_population_planned_age_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000815.json"),
            r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyCohort",
      "Join Type": "left",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      {
        "any": [
          {
            "all": [
              {
                "any": [
                  { "name": "plannedAge", "operator": "not_exists" },
                  { "name": "plannedAge", "operator": "empty" },
                  { "name": "plannedAge", "operator": "equal_to", "value": false }
                ]
              },
              {
                "any": [
                  { "name": "plannedAge.StudyCohort", "operator": "not_exists" },
                  { "name": "plannedAge.StudyCohort", "operator": "empty" },
                  { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": false }
                ]
              }
            ]
          },
          {
            "all": [
              { "name": "plannedAge", "operator": "equal_to", "value": true },
              { "name": "plannedAge.StudyCohort", "operator": "equal_to", "value": true }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Planned age must be specified either in the study population or in all cohorts.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "plannedAge", "id.StudyCohort", "name.StudyCohort", "plannedAge.StudyCohort"]
  }
}"#,
        )
        .expect("write planned age rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["population", "population", "population", "population"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "name": ["Neither", "Both", "Cohort only", "Population only"],
        "plannedAge": [false, true, false, true],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyCohort.csv",
      "domain": "StudyCohort",
      "records": {
        "parent_entity": ["StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation", "StudyDesignPopulation"],
        "parent_id": ["Population_1", "Population_2", "Population_3", "Population_4"],
        "parent_rel": ["cohorts", "cohorts", "cohorts", "cohorts"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Cohort_1", "Cohort_2", "Cohort_3", "Cohort_4"],
        "name": ["Neither cohort", "Both cohort", "Cohort age", "No cohort age"],
        "plannedAge": [false, true, true, false],
        "instanceType": ["StudyCohort", "StudyCohort", "StudyCohort", "StudyCohort"]
      }
    }
  ]
}"#,
        )
        .expect("write planned age data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_alias_code_to_standard_code_alias_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000828.json"),
            r#"{
  "Core": { "Id": "CORE-000828", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["AliasCode"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Code",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "AliasCode" },
      { "name": "parent_rel.Code", "operator": "equal_to", "value": "standardCodeAliases", "value_is_literal": true },
      { "name": "standardCode.codeSystem", "operator": "equal_to_case_insensitive", "value": "codeSystem" },
      { "name": "standardCode.codeSystemVersion", "operator": "equal_to_case_insensitive", "value": "codeSystemVersion" },
      {
        "any": [
          { "name": "standardCode.code", "operator": "equal_to_case_insensitive", "value": "code" },
          { "name": "standardCode.decode", "operator": "equal_to_case_insensitive", "value": "decode" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The standard code alias is the same as the standard code.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "standardCode.codeSystem", "standardCode.codeSystemVersion", "standardCode.code", "standardCode.decode", "codeSystem", "codeSystemVersion", "code", "decode"]
  }
}"#,
        )
        .expect("write alias code rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "AliasCode.csv",
      "domain": "AliasCode",
      "records": {
        "parent_entity": ["StudyVersion", "BiomedicalConceptProperty"],
        "parent_id": ["StudyVersion_1", "BiomedicalConceptProperty_1"],
        "parent_rel": ["studyPhase", "code"],
        "rel_type": ["definition", "definition"],
        "id": ["AliasCode_1", "AliasCode_2"],
        "instanceType": ["AliasCode", "AliasCode"],
        "standardCode.code": ["C15601", "C25208"],
        "standardCode.codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org"],
        "standardCode.codeSystemVersion": ["2023-12-15", "2023-12-15"],
        "standardCode.decode": ["Phase II Trial", "WEIGHT"]
      }
    },
    {
      "filename": "Code.csv",
      "domain": "Code",
      "records": {
        "parent_entity": ["AliasCode", "AliasCode", "AliasCode", "AliasCode"],
        "parent_id": ["AliasCode_1", "AliasCode_1", "AliasCode_2", "AliasCode_2"],
        "parent_rel": ["standardCode", "standardCodeAliases", "standardCode", "standardCodeAliases"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "id": ["Code_1", "Code_2", "Code_3", "Code_4"],
        "code": ["C15601", "c15601", "C25208", "C99904x3"],
        "codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org"],
        "codeSystemVersion": ["2023-12-15", "2023-12-15", "2023-12-15", "2023-12-15"],
        "decode": ["Phase II Trial", "Different label", "WEIGHT", "Weight"],
        "instanceType": ["Code", "Code", "Code", "Code"]
      }
    }
  ]
}"#,
        )
        .expect("write alias code data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_scheduled_activity_for_fixed_reference_timing_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "Fixed reference timing must be related to a scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "type.code", "relativeFromScheduledInstanceId", "id.ScheduledActivityInstance"]
  }
}"#,
        )
        .expect("write fixed reference timing rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "Timing.csv",
      "domain": "Timing",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["timings", "timings", "timings"],
        "rel_type": ["definition", "definition", "definition"],
        "id": ["Timing_1", "Timing_2", "Timing_3"],
        "name": ["Missing from", "Bad from", "Good from"],
        "type.code": ["C201358", "C201358", "C201358"],
        "type.decode": ["Fixed Reference", "Fixed Reference", "Fixed Reference"],
        "relativeFromScheduledInstanceId": ["", "ScheduledDecisionInstance_1", "ScheduledActivityInstance_1"],
        "instanceType": ["Timing", "Timing", "Timing"]
      }
    },
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["Timing"],
        "parent_id": ["Timing_3"],
        "parent_rel": ["relativeFromScheduledInstanceId"],
        "rel_type": ["reference"],
        "id": ["ScheduledActivityInstance_1"],
        "name": ["Dose"],
        "instanceType": ["ScheduledActivityInstance"]
      }
    }
  ]
}"#,
        )
        .expect("write fixed reference timing data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_scheduled_activity_from_open_rules_csv() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000830.json"),
            r#"{
  "Core": { "Id": "CORE-000830", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Timing"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Join Type": "left",
      "Keys": [
        { "Left": "relativeFromScheduledInstanceId", "Right": "id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "type.code", "operator": "equal_to", "value": "C201358" },
      {
        "any": [
          { "name": "relativeFromScheduledInstanceId", "operator": "empty" },
          { "name": "id.ScheduledActivityInstance", "operator": "not_exists" },
          { "name": "id.ScheduledActivityInstance", "operator": "empty" }
        ]
      }
    ]
  },
  "Outcome": { "Message": "Fixed reference timing must be related to a scheduled activity instance." }
}"#,
        )
        .expect("write fixed reference timing rule");

    fs::write(
            data_dir.join("_datasets.csv"),
            "Filename,Dataset Name,Label\nTiming,Timing,Timing\nScheduledActivityInstance,ScheduledActivityInstance,Scheduled Activity Instance\n",
        )
        .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nTiming,parent_entity,Parent Entity Name,String,[1]\nTiming,parent_id,Parent Entity Id,String,[1]\nTiming,parent_rel,Name of Relationship from Parent Entity,String,[1]\nTiming,rel_type,Type of Relationship,String,[1]\nTiming,id,Timing Id,String,[1]\nTiming,name,Timing Name,String,[1]\nTiming,type.code,Timing Type Code,String,[1]\nTiming,relativeFromScheduledInstanceId,Timing Relative From Scheduled Instance,String,ScheduledInstance[0..1].id[1]\nTiming,instanceType,Instance Type,String,[1]\nScheduledActivityInstance,parent_entity,Parent Entity Name,String,[1]\nScheduledActivityInstance,parent_id,Parent Entity Id,String,[1]\nScheduledActivityInstance,parent_rel,Name of Relationship from Parent Entity,String,[1]\nScheduledActivityInstance,rel_type,Type of Relationship,String,[1]\nScheduledActivityInstance,id,Scheduled Activity Instance Id,String,[1]\nScheduledActivityInstance,name,Scheduled Activity Instance Name,String,[1]\nScheduledActivityInstance,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("Timing.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,type.code,relativeFromScheduledInstanceId,instanceType\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_1,Missing from,C201358,,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_2,Bad from,C201358,ScheduledDecisionInstance_1,Timing\nScheduleTimeline,ScheduleTimeline_1,timings,definition,Timing_3,Good from,C201358,ScheduledActivityInstance_1,Timing\n",
        )
        .expect("write timing csv");
    fs::write(
            data_dir.join("ScheduledActivityInstance.csv"),
            "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType\nTiming,Timing_3,relativeFromScheduledInstanceId,reference,ScheduledActivityInstance_1,Dose,ScheduledActivityInstance\n",
        )
        .expect("write scheduled activity csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_left_joins_objective_for_primary_endpoint_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000874.json"),
            r#"{
  "Core": { "Id": "CORE-000874", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["Endpoint"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Objective",
      "Join Type": "left",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "level.code", "operator": "equal_to", "value": "C94496" },
      { "name": "level.code.Objective", "operator": "not_equal_to", "value": "C85826" }
    ]
  },
  "Outcome": {
    "Message": "The primary endpoint (level.code = C94496) is not referenced by a primary objective (level.code.Objective = C85826).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "level.code", "name.Objective", "level.code.Objective"]
  }
}"#,
        )
        .expect("write primary endpoint objective rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "Endpoint.csv",
      "domain": "Endpoint",
      "records": {
        "parent_entity": ["Objective", "Objective"],
        "parent_id": ["Objective_1", "Objective_2"],
        "parent_rel": ["endpoints", "endpoints"],
        "rel_type": ["definition", "definition"],
        "id": ["Endpoint_1", "Endpoint_2"],
        "name": ["Primary bad", "Primary good"],
        "level.code": ["C94496", "C94496"],
        "level.decode": ["Primary", "Primary"],
        "instanceType": ["Endpoint", "Endpoint"]
      }
    },
    {
      "filename": "Objective.csv",
      "domain": "Objective",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["objectives", "objectives"],
        "rel_type": ["definition", "definition"],
        "id": ["Objective_1", "Objective_2"],
        "name": ["Secondary objective", "Primary objective"],
        "level.code": ["C85827", "C85826"],
        "level.decode": ["Secondary", "Primary"],
        "instanceType": ["Objective", "Objective"]
      }
    }
  ]
}"#,
    )
    .expect("write primary endpoint objective data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_left_joins_study_epochs_for_study_arm_cell_coverage() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000797.json"),
            r#"{
  "Core": { "Id": "CORE-000797", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Join Type": "left",
      "Keys": ["parent_entity", "parent_id", "rel_type"]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyArm" },
      { "name": "id.StudyEpoch", "operator": "is_unique_set", "value": "id" },
      {
        "any": [
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
              { "name": "rel_type", "operator": "equal_to", "value": "definition" },
              { "name": "parent_rel", "operator": "equal_to", "value": "arms" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochs" }
            ]
          },
          {
            "all": [
              { "name": "parent_entity", "operator": "equal_to", "value": "StudyCell" },
              { "name": "rel_type", "operator": "equal_to", "value": "reference" },
              { "name": "parent_rel", "operator": "equal_to", "value": "armId" },
              { "name": "parent_rel.StudyEpoch", "operator": "equal_to", "value": "epochId" }
            ]
          }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "The StudyArm does not have a StudyCell for the StudyEpoch.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "id.StudyEpoch", "name.StudyEpoch"]
  }
}"#,
        )
        .expect("write study arm coverage rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["arms", "arms", "armId", "armId", "armId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyArm_1", "StudyArm_2", "StudyArm_1", "StudyArm_1", "StudyArm_2"],
        "name": ["Placebo", "Active", "Placebo", "Placebo", "Active"],
        "instanceType": ["StudyArm", "StudyArm", "StudyArm", "StudyArm", "StudyArm"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign", "StudyCell", "StudyCell", "StudyCell"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1", "StudyCell_1", "StudyCell_2", "StudyCell_3"],
        "parent_rel": ["epochs", "epochs", "epochId", "epochId", "epochId"],
        "rel_type": ["definition", "definition", "reference", "reference", "reference"],
        "id": ["StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1", "StudyEpoch_2", "StudyEpoch_1"],
        "name": ["Screening", "Treatment", "Screening", "Treatment", "Screening"],
        "instanceType": ["StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
        )
        .expect("write study arm coverage data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_activity_for_duplicate_biomedical_category_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000811.json"),
            r#"{
  "Core": { "Id": "CORE-000811", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConceptCategory"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "Activity",
      "Keys": [
        { "Left": "parent_id", "Right": "id" },
        { "Left": "parent_entity", "Right": "instanceType" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConceptCategory" },
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "rel_type.Activity", "operator": "equal_to", "value": "definition" },
      { "name": "parent_entity", "operator": "equal_to", "value": "Activity" },
      { "name": "parent_rel", "operator": "equal_to", "value": "bcCategoryIds", "value_is_literal": true },
      {
        "name": "id",
        "operator": "is_not_unique_set",
        "value": ["parent_entity", "parent_id", "parent_rel", "rel_type.Activity"]
      }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept category is referenced more than once from the same activity.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "name.Activity"]
  }
}"#,
        )
        .expect("write duplicate biomedical category rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "BiomedicalConceptCategory.csv",
      "domain": "BiomedicalConceptCategory",
      "records": {
        "parent_entity": ["Activity", "Activity", "Activity"],
        "parent_id": ["Activity_1", "Activity_1", "Activity_1"],
        "parent_rel": ["bcCategoryIds", "bcCategoryIds", "bcCategoryIds"],
        "rel_type": ["reference", "reference", "reference"],
        "id": ["BiomedicalConceptCategory_1", "BiomedicalConceptCategory_1", "BiomedicalConceptCategory_2"],
        "name": ["Vital Signs", "Vital Signs", "Labs"],
        "instanceType": ["BiomedicalConceptCategory", "BiomedicalConceptCategory", "BiomedicalConceptCategory"]
      }
    },
    {
      "filename": "Activity.csv",
      "domain": "Activity",
      "records": {
        "parent_entity": ["StudyDesign"],
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["activities"],
        "rel_type": ["definition"],
        "id": ["Activity_1"],
        "name": ["Vital signs tests"],
        "instanceType": ["Activity"]
      }
    }
  ]
}"#,
        )
        .expect("write duplicate biomedical category data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_joins_string_synonym_for_biomedical_concept_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000803.json"),
            r#"{
  "Core": { "Id": "CORE-000803", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["BiomedicalConcept"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "string",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "BiomedicalConcept" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_rel.string", "operator": "equal_to", "value": "synonyms", "value_is_literal": true },
      { "name": "value", "operator": "equal_to_case_insensitive", "value": "name" }
    ]
  },
  "Outcome": {
    "Message": "The biomedical concept synonym value is the same as the biomedical concept name (case insensitive).",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "parent_rel.string", "value"]
  }
}"#,
        )
        .expect("write biomedical concept synonym rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "BiomedicalConcept.csv",
      "domain": "BiomedicalConcept",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["biomedicalConcepts", "biomedicalConcepts"],
        "rel_type": ["definition", "definition"],
        "id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "name": ["Race", "Weight"],
        "instanceType": ["BiomedicalConcept", "BiomedicalConcept"]
      }
    },
    {
      "filename": "string.csv",
      "domain": "string",
      "records": {
        "parent_entity": ["BiomedicalConcept", "BiomedicalConcept"],
        "parent_id": ["BiomedicalConcept_1", "BiomedicalConcept_2"],
        "parent_rel": ["synonyms", "synonyms"],
        "rel_type": ["definition", "definition"],
        "value": ["race", "Mass"]
      }
    }
  ]
}"#,
    )
    .expect("write biomedical concept synonym data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_timeline_exit_parent_for_scheduled_activity_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000819.json"),
            r#"{
  "Core": { "Id": "CORE-000819", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduleTimelineExit",
      "Keys": [
        { "Left": "timelineExitId", "Right": "id" },
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "ScheduledActivityInstance" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "timelineExitId", "operator": "non_empty" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.ScheduleTimelineExit" }
    ]
  },
  "Outcome": {
    "Message": "The scheduled activity instance references a timeline exit that is not defined within the same schedule timeline as the scheduled activity instance.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "name", "timelineExitId", "parent_id.ScheduleTimelineExit"]
  }
}"#,
        )
        .expect("write timeline exit match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ScheduledActivityInstance.csv",
      "domain": "ScheduledActivityInstance",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_1"],
        "parent_rel": ["instances", "instances"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduledActivityInstance_1", "ScheduledActivityInstance_2"],
        "name": ["OK", "BAD"],
        "timelineExitId": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduledActivityInstance", "ScheduledActivityInstance"]
      }
    },
    {
      "filename": "ScheduleTimelineExit.csv",
      "domain": "ScheduleTimelineExit",
      "records": {
        "parent_entity": ["ScheduleTimeline", "ScheduleTimeline"],
        "parent_id": ["ScheduleTimeline_1", "ScheduleTimeline_2"],
        "parent_rel": ["exits", "exits"],
        "rel_type": ["definition", "definition"],
        "id": ["ScheduleTimelineExit_1", "ScheduleTimelineExit_2"],
        "instanceType": ["ScheduleTimelineExit", "ScheduleTimelineExit"]
      }
    }
  ]
}"#,
    )
    .expect("write timeline exit match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_study_arm_parent_for_study_cell_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000835.json"),
            r#"{
  "Core": { "Id": "CORE-000835", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "armId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyArm" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an arm that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "armId", "parent_id.StudyArm"]
  }
}"#,
        )
        .expect("write study arm match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "armId": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["arms", "arms"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyArm_1", "StudyArm_2"],
        "instanceType": ["StudyArm", "StudyArm"]
      }
    }
  ]
}"#,
    )
    .expect("write study arm match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_study_epoch_parent_for_study_cell_scope() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000836.json"),
            r#"{
  "Core": { "Id": "CORE-000836", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyEpoch",
      "Keys": [
        { "Left": "epochId", "Right": "id" },
        "parent_entity",
        "rel_type"
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyCell" },
      { "name": "parent_entity", "operator": "equal_to", "value": "StudyDesign" },
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id", "operator": "not_equal_to", "value": "parent_id.StudyEpoch" }
    ]
  },
  "Outcome": {
    "Message": "The study cell references an epoch that is not defined within the same study design as the study cell.",
    "Output Variables": ["parent_entity", "parent_id", "parent_rel", "id", "epochId", "parent_id.StudyEpoch"]
  }
}"#,
        )
        .expect("write study epoch match rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyCell.csv",
      "domain": "StudyCell",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_1"],
        "parent_rel": ["cells", "cells"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyCell_1", "StudyCell_2"],
        "epochId": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyCell", "StudyCell"]
      }
    },
    {
      "filename": "StudyEpoch.csv",
      "domain": "StudyEpoch",
      "records": {
        "parent_entity": ["StudyDesign", "StudyDesign"],
        "parent_id": ["StudyDesign_1", "StudyDesign_2"],
        "parent_rel": ["epochs", "epochs"],
        "rel_type": ["definition", "definition"],
        "id": ["StudyEpoch_1", "StudyEpoch_2"],
        "instanceType": ["StudyEpoch", "StudyEpoch"]
      }
    }
  ]
}"#,
    )
    .expect("write study epoch match data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_joins_single_match_dataset_to_each_scoped_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-MULTI-BASE-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-MULTI-BASE-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "RFSTDTC",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Reference start date must be reviewed" }
}"#,
    )
    .expect("write match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"],
        "DOMAIN": ["AE"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S2"],
        "DOMAIN": ["CM"],
        "CMSEQ": [1]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1", "S2"],
        "RFSTDTC": ["BAD", "OK"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let failed = outcome
        .results
        .iter()
        .find(|result| result.dataset == "AE")
        .expect("AE result");
    assert_eq!(failed.execution_status, ExecutionStatus::Failed);
    assert_eq!(failed.error_count, 1);
    let passed = outcome
        .results
        .iter()
        .find(|result| result.dataset == "CM")
        .expect("CM result");
    assert_eq!(passed.execution_status, ExecutionStatus::Passed);
}

#[test]
fn run_validation_reports_multi_base_match_dataset_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000354.json"),
        r#"{
  "Core": { "Id": "CORE-000354", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "CM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "RFSTDTC",
    "operator": "equal_to",
    "value": "BAD"
  },
  "Outcome": { "Message": "Reference start date has oracle-specific join semantics" }
}"#,
    )
    .expect("write match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["BAD"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Failed));
    assert!(outcome
        .results
        .iter()
        .all(|result| result.skipped_reason.is_none()));
}

#[test]
fn run_validation_skips_usdm_match_dataset_oracle_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000815.json"),
        r#"{
  "Core": { "Id": "CORE-000815", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ScheduleTimeline"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "ScheduledActivityInstance",
      "Keys": [
        { "Left": "id", "Right": "parent_id" },
        { "Left": "instanceType", "Right": "parent_entity" }
      ]
    }
  ],
  "Check": {
    "name": "instanceType",
    "operator": "equal_to",
    "value": "ScheduleTimeline"
  },
  "Outcome": { "Message": "USDM match dataset has oracle-specific flatten semantics" }
}"#,
    )
    .expect("write USDM match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ScheduleTimeline.csv",
      "domain": "ScheduleTimeline",
      "records": {
        "id": ["ScheduleTimeline_1"],
        "instanceType": ["ScheduleTimeline"]
      }
    }
  ]
}"#,
    )
    .expect("write USDM match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::DatasetJoinNotSupported)
    );
}

#[test]
fn run_validation_fans_out_single_match_dataset_with_duplicate_lookup_keys() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DUPLICATE-MATCH-DATASET.json"),
        r#"{
  "Core": { "Id": "CORE-DUPLICATE-MATCH-DATASET", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "LOOKUP", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Lookup flag must not be Y" }
}"#,
    )
    .expect("write duplicate match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "DOMAIN": ["DM"]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S1", "S1"],
        "FLAG": ["Y", "N"]
      }
    }
  ]
}"#,
    )
    .expect("write duplicate match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
}

#[test]
fn run_validation_reports_duplicate_match_dataset_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000252.json"),
        r#"{
  "Core": { "Id": "CORE-000252", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DS", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "name": "DSDECOD",
    "operator": "equal_to",
    "value": "DEATH"
  },
  "Outcome": { "Message": "Death disposition has oracle-specific duplicate match semantics" }
}"#,
    )
    .expect("write duplicate match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"]
      }
    },
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["S1", "S1"],
        "DSDECOD": ["DEATH", "COMPLETED"]
      }
    }
  ]
}"#,
    )
    .expect("write duplicate match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_skips_relrec_and_supp_match_dataset_oracle_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000206.json"),
        r#"{
  "Core": { "Id": "CORE-000206", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SUPP--", "Keys": ["USUBJID", "IDVAR", "IDVARVAL"] },
    { "Name": "RELREC", "Keys": ["USUBJID", "IDVAR", "IDVARVAL"] }
  ],
  "Check": {
    "name": "IDVARVAL",
    "operator": "not_equal_to",
    "value": ""
  },
  "Outcome": { "Message": "SUPP-- placeholder has oracle-specific match semantics" }
}"#,
    )
    .expect("write supp placeholder rule");
    fs::write(
        rules_dir.join("CORE-000744.json"),
        r#"{
  "Core": { "Id": "CORE-000744", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["FA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "RELREC" }
  ],
  "Check": {
    "name": "FAOBJ",
    "operator": "not_equal_to",
    "value": "RELREC.AETERM"
  },
  "Outcome": { "Message": "RELREC wildcard has oracle-specific match semantics" }
}"#,
    )
    .expect("write relrec rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "USUBJID": ["S1"],
        "IDVAR": ["AESEQ"],
        "IDVARVAL": ["1"]
      }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FAOBJ": ["TERM"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "USUBJID": ["S1"],
        "RELID": ["R1"]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Skipped));
    assert!(outcome
        .results
        .iter()
        .all(|result| result.skipped_reason == Some(SkippedReason::OracleSemanticsGap)));
}

#[test]
fn run_validation_uses_reference_distinct_operation_values_as_sets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-REFERENCE-DISTINCT.json"),
        r#"{
  "Core": { "Id": "CORE-REFERENCE-DISTINCT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["RELREC"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "RELREC",
      "id": "$rdomain_variables",
      "name": "IDVAR",
      "operator": "distinct",
      "value_is_reference": true
    }
  ],
  "Check": {
    "all": [
      { "name": "RDOMAIN", "operator": "exists" },
      { "name": "IDVAR", "operator": "non_empty" },
      {
        "name": "IDVAR",
        "operator": "is_not_contained_by",
        "value": "$rdomain_variables"
      }
    ]
  },
  "Outcome": { "Message": "IDVAR must name a variable in RDOMAIN" }
}"#,
    )
    .expect("write reference distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "variables": [
        { "name": "STUDYID" },
        { "name": "RDOMAIN" },
        { "name": "USUBJID" },
        { "name": "IDVAR" },
        { "name": "IDVARVAL" }
      ],
      "records": {
        "STUDYID": ["S1", "S1"],
        "RDOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "IDVAR": ["LBSEQ", "AESEQ"],
        "IDVARVAL": ["1", "2"]
      }
    },
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "variables": [
        { "name": "STUDYID" },
        { "name": "USUBJID" },
        { "name": "LBSEQ" },
        { "name": "LBTESTCD" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBTESTCD": ["ALT"]
      }
    }
  ]
}"#,
    )
    .expect("write reference distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_core_000172_reference_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000172.json"),
        r#"{
  "Core": { "Id": "CORE-000172", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_studyid",
      "name": "STUDYID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "STUDYID",
    "operator": "is_not_contained_by",
    "value": "$dm_studyid"
  },
  "Outcome": { "Message": "STUDYID is not equal to DM.STUDYID" }
}"#,
    )
    .expect("write distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"]
      }
    }
  ]
}"#,
    )
    .expect("write distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_skips_core_000172_sendig_reference_distinct_oracle_gap() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000172.json"),
        r#"{
  "Core": { "Id": "CORE-000172", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": { "Include": ["AE", "DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_studyid",
      "name": "STUDYID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "STUDYID",
    "operator": "is_not_contained_by",
    "value": "$dm_studyid"
  },
  "Outcome": { "Message": "STUDYID is not equal to DM.STUDYID" }
}"#,
    )
    .expect("write SENDIG distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"]
      }
    }
  ]
}"#,
    )
    .expect("write SENDIG distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000172".to_owned()],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_executes_core_000201_reference_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000201.json"),
        r#"{
  "Core": { "Id": "CORE-000201", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE", "DM", "TA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "id": "$dm_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "non_empty" },
      { "name": "USUBJID", "operator": "is_not_contained_by", "value": "$dm_usubjid" }
    ]
  },
  "Outcome": { "Message": "USUBJID is not found in DM.USUBJID" }
}"#,
    )
    .expect("write distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"]
      }
    },
    {
      "filename": "ta.xpt",
      "domain": "TA",
      "records": {
        "STUDYID": ["S1"],
        "ARMCD": ["A"],
        "ARM": ["Active"]
      }
    }
  ]
}"#,
    )
    .expect("write distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let by_dataset = outcome
        .results
        .iter()
        .map(|result| (result.dataset.as_str(), result))
        .collect::<std::collections::BTreeMap<_, _>>();

    let ae = by_dataset.get("AE").expect("AE result");
    assert_eq!(ae.execution_status, ExecutionStatus::Failed, "{ae:?}");
    assert_eq!(ae.error_count, 1);
    assert_eq!(ae.errors[0].row, Some(2));
    assert_eq!(ae.errors[0].seq.as_deref(), Some("2"));

    let ta = by_dataset.get("TA").expect("TA result");
    assert_eq!(ta.execution_status, ExecutionStatus::Failed, "{ta:?}");
    assert_eq!(ta.error_count, 1);
    assert_eq!(ta.errors[0].row, None);
    assert!(ta.errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_core_000239_external_min_date_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000239.json"),
        r#"{
  "Core": { "Id": "CORE-000239", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$min_ex_exstdtc",
      "name": "EXSTDTC",
      "operator": "min_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXSTDTC", "operator": "not_equal_to", "value": "$min_ex_exstdtc" }
    ]
  },
  "Outcome": {
    "Message": "RFXSTDTC does not equal the earliest value of EX.EXSTDTC",
    "Output Variables": ["RFXSTDTC", "$min_ex_exstdtc"]
  }
}"#,
    )
    .expect("write min date rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXSTDTC": ["2020-01-02", "2020-02-03"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "EXSTDTC": ["2020-01-03", "2020-01-02", "2020-02-01"]
      }
    }
  ]
}"#,
    )
    .expect("write min date data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_core_000238_external_max_date_operations() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000238.json"),
        r#"{
  "Core": { "Id": "CORE-000238", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_exstdtc",
      "name": "EXSTDTC",
      "operator": "max_date"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_exendtc",
      "name": "EXENDTC",
      "operator": "max_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exstdtc" },
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exendtc" }
    ]
  },
  "Outcome": {
    "Message": "RFXENDTC does not equal the latest value of EX.EXSTDTC or EX.EXENDTC",
    "Output Variables": ["RFXENDTC", "$max_ex_exstdtc", "$max_ex_exendtc"]
  }
}"#,
    )
    .expect("write max date rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXENDTC": ["2020-01-05", "2020-02-04"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "EXSTDTC": ["2020-01-01", "2020-01-03", "2020-02-01", "2020-02-02"],
        "EXENDTC": ["2020-01-02", "2020-01-05", "2020-02-02", "2020-02-03"]
      }
    }
  ]
}"#,
    )
    .expect("write max date data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");
    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_grouped_min_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-MIN.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-MIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "source_column": "AVAL",
      "as": "MIN_AVAL",
      "by": ["USUBJID"],
      "operator": "min"
    }
  ],
  "Check": {
    "name": "MIN_AVAL",
    "operator": "equal_to",
    "value": 3
  },
  "Outcome": {
    "Message": "AVAL is not the subject-wise minimum"
  }
}"#,
    )
    .expect("write min operation rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "AVAL": [3, 4, 5]
      }
    }
  ]
}"#,
    )
    .expect("write min operation data");

    let dataset_path_for_validation = dataset_path.clone();
    let rules_dir_for_validation = rules_dir.clone();
    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir_for_validation],
        dataset_paths: vec![dataset_path_for_validation],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_grouped_max_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-MAX.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-MAX", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DM",
      "source_column": "AVAL",
      "as": "MAX_AVAL",
      "by": ["USUBJID"],
      "operator": "max"
    }
  ],
  "Check": {
    "name": "MAX_AVAL",
    "operator": "equal_to",
    "value": 10
  },
  "Outcome": {
    "Message": "AVAL is not the subject-wise maximum"
  }
}"#,
    )
    .expect("write max operation rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "AVAL": [2, 10, 5, 3]
      }
    }
  ]
}"#,
    )
    .expect("write max operation data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_grouped_external_min_max_operations() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-MIN-MAX-EXT.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-MIN-MAX-EXT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "EX",
      "id": "$ex_usubjid",
      "name": "USUBJID",
      "operator": "distinct"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$min_ex_aval",
      "name": "AVAL",
      "operator": "min"
    },
    {
      "domain": "EX",
      "group": ["USUBJID"],
      "id": "$max_ex_aval",
      "name": "AVAL",
      "operator": "max"
    }
  ],
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "is_contained_by", "value": "$ex_usubjid" },
      { "name": "RFXMIN", "operator": "not_equal_to", "value": "$min_ex_aval" },
      { "name": "RFXMAX", "operator": "not_equal_to", "value": "$max_ex_aval" }
    ]
  },
  "Outcome": {
    "Message": "RFX values are not equal to grouped EX AVAL"
  }
}"#,
    )
    .expect("write external min max operation rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RFXMIN": [9, 0],
        "RFXMAX": [5, 9]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S2"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ3"],
        "AVAL": [5, 2, 7, 1]
      }
    }
  ]
}"#,
    )
    .expect("write external min max operation data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_reports_core_000773_date_operation_gap_failure() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000773.json"),
        r#"{
  "Core": { "Id": "CORE-000773", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MA"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "DS",
      "group": ["USUBJID"],
      "id": "$dsstdtc",
      "name": "DSSTDTC",
      "operator": "max_date"
    }
  ],
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "date_greater_than", "value": "$dsstdtc" }
    ]
  },
  "Outcome": { "Message": "--DTC may not be later than DS.DSSTDTC" }
}"#,
    )
    .expect("write date gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ma.xpt",
      "domain": "MA",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "MADTC": ["2020-01-02T00:00:01"]
      }
    },
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "DSSTDTC": ["2020-01-02T00:00:00"]
      }
    }
  ]
}"#,
    )
    .expect("write date gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_executes_core_000770_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000770.json"),
        r#"{
  "Core": { "Id": "CORE-000770", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "distinct",
      "domain": "TX",
      "group": ["SETCD"],
      "name": "TXPARMCD",
      "id": "$txparms_by_set"
    }
  ],
  "Check": { "name": "$txparms_by_set", "operator": "does_not_contain", "value": "SPGRPCD" },
  "Outcome": { "Message": "TXPARMCD must include SPGRPCD per SETCD" }
}"#,
    )
    .expect("write distinct operation gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2"],
        "SETCD": ["SET1", "SET1", "SET2"],
        "TXPARMCD": ["ARMCD", "SPGRPCD", "ARMCD"]
      }
    }
  ]
}"#,
    )
    .expect("write distinct date gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(3));
}

#[test]
fn run_validation_executes_scope_wide_reference_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000140.json"),
        r#"{
  "Core": { "Id": "CORE-000140", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" },
      { "name": "VISITDY", "operator": "non_empty" }
    ]
  },
  "Outcome": {
    "Message": "VISITDY is populated for an unplanned visit",
    "Output Variables": ["VISITNUM", "VISITDY"]
  }
}"#,
    )
    .expect("write distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [1, 2]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [1, 99],
        "VISITDY": [1, 99]
      }
    }
  ]
}"#,
    )
    .expect("write distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .find(|result| result.execution_status == ExecutionStatus::Failed)
        .expect("failed result");
    assert_eq!(failed.dataset, "SV");
    assert_eq!(failed.error_count, 1);
    assert_eq!(failed.errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_core_000361_one_way_relationship_semantics() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000361.json"),
        r#"{
  "Core": { "Id": "CORE-000361", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV", "TV"] }, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "TV", "id": "$tv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": { "all": [
    { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" },
    { "name": "VISIT", "operator": "is_not_unique_relationship", "value": "VISITNUM" }
  ] },
  "Outcome": {
    "Message": "VISIT and VISITNUM do not have a one-to-one relationship",
    "Output Variables": ["VISITNUM", "VISIT"]
  }
}"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": [700, 700],
        "VISIT": ["VISIT 7 (WEEK 5)", "VISIT 8 (WEEK 6)"]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "SVSEQ": [1, 1],
        "VISITNUM": [100, 100],
        "VISIT": ["VISIT 1 (WEEK -2)", "VISIT 1"]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000361");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_passes_core_000678_when_pooldef_is_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000678.yml"),
        r#"
Core:
  Id: CORE-000678
  Status: Published
Scope:
  Classes:
    Include:
      - ALL
  Domains:
    Include:
      - ALL
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: POOLDEF
    id: $pooldef_poolid
    name: POOLID
    operator: distinct
Check:
  all:
    - name: POOLID
      operator: non_empty
    - name: POOLID
      operator: is_not_contained_by
      value: $pooldef_poolid
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - POOLID
    - $pooldef_poolid
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["VS", "VS"],
        "USUBJID": ["", ""],
        "POOLID": ["POOL1", "POOL2"],
        "VSSEQ": [1, 2]
      }
    }
  ]
}"#,
    )
    .expect("write data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_executes_domain_label_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DOMAIN-LABEL.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-LABEL", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "Category must not repeat the domain label" }
}"#,
    )
    .expect("write domain label rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "LBCAT": ["Laboratory Test Results", "LB", "CHEMISTRY"]
      }
    }
  ]
}"#,
    )
    .expect("write domain label data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000272_domain_label_cat_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000272.json"),
        r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
    )
    .expect("write domain label rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "LBCAT": ["Laboratory Test Results"]
      }
    }
  ]
}"#,
    )
    .expect("write domain label oracle gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000272".to_owned()],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000272_sendig_domain_name_cat_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000272.json"),
        r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
    )
    .expect("write SENDIG domain name rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBCAT": ["LB"]
      }
    }
  ]
}"#,
    )
    .expect("write SENDIG domain name data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000272".to_owned()],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["LBCAT".to_owned(), "DOMAIN".to_owned()]
    );
}

#[test]
fn run_validation_executes_extract_metadata_dataset_name_string_part_check() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-EXTRACT-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-EXTRACT-METADATA", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$dataset_name",
      "name": "dataset_name",
      "operator": "extract_metadata"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "does_not_equal_string_part",
    "regex": ".{4}(..).*",
    "value": "$dataset_name"
  },
  "Outcome": { "Message": "RDOMAIN must match the parent domain in the SUPP dataset name" }
}"#,
    )
    .expect("write extract metadata rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "RDOMAIN": ["AE", "XX"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "QNAM": ["AETERM", "BAD"]
      }
    }
  ]
}"#,
    )
    .expect("write extract metadata data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_get_xhtml_errors_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-XHTML.json"),
        r#"{
  "Core": { "Id": "CORE-XHTML", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["EligibilityCriterion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$xhtml_errors",
      "name": "text",
      "namespace": "http://www.cdisc.org/ns/usdm/xhtml/v1.0",
      "operator": "get_xhtml_errors"
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "$xhtml_errors", "operator": "non_empty" }
    ]
  },
  "Outcome": { "Message": "The text attribute contains non-conformant XHTML." }
}"#,
    )
    .expect("write xhtml rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "EligibilityCriterion.csv",
      "domain": "EligibilityCriterion",
      "records": {
        "rel_type": ["definition", "definition", "definition", "label"],
        "name": ["VALID", "BAD_TAG", "BAD_XML", "IGNORED"],
        "text": [
          "<p>At least <usdm:tag name=\"min_age\"/> years.</p>",
          "<p><usdm:tag nam=\"min_age\"/></p>",
          "Insulin-dependent & diabetic",
          "Insulin-dependent & diabetic"
        ]
      }
    }
  ]
}"#,
    )
    .expect("write xhtml data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[1].row, Some(3));
}

#[test]
fn run_validation_executes_reference_distinct_operation_from_scope_external_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000036.json"),
        r#"{
  "Core": { "Id": "CORE-000036", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visit",
      "name": "VISIT",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISIT", "operator": "is_not_contained_by", "value": "$tv_visit" }
    ]
  },
  "Outcome": { "Message": "Planned visit is not found in TV" }
}"#,
    )
    .expect("write reference distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "SVSEQ": [1, 2, 3],
        "SVPRESP": ["Y", "Y", "N"],
        "VISIT": ["BASELINE", "SCREENING", "SCREENING"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "VISIT": ["BASELINE", "WEEK 1"]
      }
    }
  ]
}"#,
    )
    .expect("write reference distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["SVPRESP".to_owned(), "VISIT".to_owned()]
    );
}

#[test]
fn run_validation_executes_tv_visitnum_reference_distinct_operations() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000039.json"),
        r#"{
  "Core": { "Id": "CORE-000039", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visitnum",
      "name": "VISITNUM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": { "Message": "Planned visit number is not found in TV" }
}"#,
    )
    .expect("write planned visitnum rule");
    fs::write(
        rules_dir.join("CORE-000040.json"),
        r#"{
  "Core": { "Id": "CORE-000040", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TV",
      "id": "$tv_visitnum",
      "name": "VISITNUM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "empty" },
      { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" }
    ]
  },
  "Outcome": { "Message": "Unplanned visit number is found in TV" }
}"#,
    )
    .expect("write unplanned visitnum rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "SVSEQ": [1, 2, 3],
        "SVPRESP": ["Y", "Y", ""],
        "VISITNUM": ["1", "99", "1"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "VISITNUM": ["1", "2"]
      }
    }
  ]
}"#,
    )
    .expect("write visitnum data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let by_rule = outcome
        .results
        .iter()
        .map(|result| (result.rule_id.as_str(), result))
        .collect::<std::collections::BTreeMap<_, _>>();
    let planned = by_rule.get("CORE-000039").expect("planned result");
    assert_eq!(planned.execution_status, ExecutionStatus::Failed);
    assert_eq!(planned.error_count, 1);
    assert_eq!(planned.errors[0].row, Some(2));
    assert_eq!(planned.errors[0].seq.as_deref(), Some("2"));

    let unplanned = by_rule.get("CORE-000040").expect("unplanned result");
    assert_eq!(unplanned.execution_status, ExecutionStatus::Failed);
    assert_eq!(unplanned.error_count, 1);
    assert_eq!(unplanned.errors[0].row, Some(3));
    assert_eq!(unplanned.errors[0].seq.as_deref(), Some("3"));
}

#[test]
fn run_validation_executes_trial_arm_reference_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000047.json"),
        r#"{
  "Core": { "Id": "CORE-000047", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TA",
      "id": "$ta_arm",
      "name": "ARM",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "ACTARM", "operator": "non_empty" },
      { "name": "ARM", "operator": "non_empty" },
      { "name": "ARM", "operator": "is_not_contained_by", "value": "$ta_arm" }
    ]
  },
  "Outcome": { "Message": "DM ARM is not found in TA" }
}"#,
    )
    .expect("write arm reference distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "ARM": ["PLACEBO", "BADARM", ""],
        "ACTARM": ["PLACEBO", "BADARM", "PLACEBO"]
      }
    },
    {
      "filename": "ta.xpt",
      "domain": "TA",
      "records": {
        "STUDYID": ["S1", "S1"],
        "ARM": ["PLACEBO", "DRUG"]
      }
    }
  ]
}"#,
    )
    .expect("write arm reference distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_study_domains_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-STUDY-DOMAINS.json"),
        r#"{
  "Core": { "Id": "CORE-STUDY-DOMAINS", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["RELREC"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$study_domains",
      "operator": "study_domains"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "is_not_contained_by",
    "value": "$study_domains"
  },
  "Outcome": {
    "Message": "RDOMAIN does not represent a dataset present in the study",
    "Output Variables": ["RDOMAIN"]
  }
}"#,
    )
    .expect("write study domains rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RELID": ["R1", "R2"],
        "RDOMAIN": ["AE", "XX"]
      }
    },
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "AESEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write study domains data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["RDOMAIN".to_owned()]
    );
}

#[test]
fn run_validation_executes_variable_count_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-VARIABLE-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-VARIABLE-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$VARIABLE_COUNT",
      "name": "--LNKGRP",
      "operator": "variable_count"
    }
  ],
  "Check": {
    "all": [
      { "name": "--LNKGRP", "operator": "exists" },
      { "name": "$VARIABLE_COUNT", "operator": "less_than", "value": 2 }
    ]
  },
  "Outcome": {
    "Message": "LNKGRP variable is not found in any of the other domains.",
    "Output Variables": ["--LNKGRP", "$VARIABLE_COUNT"]
  }
}"#,
    )
    .expect("write variable count rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID" },
        { "name": "AESEQ" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID" },
        { "name": "FASEQ" },
        { "name": "FALNKGRP" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "FASEQ": [1],
        "FALNKGRP": ["CDISC001 - 1"]
      }
    }
  ]
}"#,
    )
    .expect("write variable count data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let result = outcome
        .results
        .iter()
        .find(|result| result.dataset == "FA")
        .expect("FA result");
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(
        result.errors[0].variables,
        vec!["FALNKGRP".to_owned(), "$VARIABLE_COUNT".to_owned()]
    );
}

#[test]
fn run_validation_executes_dy_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DY.json"),
        r#"{
  "Core": { "Id": "CORE-DY", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Operations": [
    {
      "id": "$val_dy",
      "name": "--STDTC",
      "operator": "dy"
    }
  ],
  "Check": {
    "all": [
      { "name": "--STDTC", "operator": "is_complete_date" },
      { "name": "RFSTDTC", "operator": "is_complete_date" },
      { "name": "--STDY", "operator": "not_equal_to", "value": "$val_dy" }
    ]
  },
  "Outcome": {
    "Message": "--DY is not calculated correctly",
    "Output Variables": ["--STDY", "--STDTC", "RFSTDTC", "$val_dy"]
  }
}"#,
    )
    .expect("write dy rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1", "S1"],
        "AESEQ": [1, 2, 3],
        "AESTDTC": ["2024-01-01", "2023-12-31", "2024-01-02"],
        "AESTDY": [1, -1, 3]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
    )
    .expect("write dy data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let result = outcome
        .results
        .iter()
        .find(|result| result.dataset == "AE")
        .unwrap_or_else(|| panic!("AE result not found: {:?}", outcome.results));
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(3));
    assert_eq!(result.errors[0].seq.as_deref(), Some("3"));
    assert_eq!(
        result.errors[0].variables,
        vec![
            "AESTDY".to_owned(),
            "AESTDTC".to_owned(),
            "RFSTDTC".to_owned(),
            "$val_dy".to_owned()
        ]
    );
}

#[test]
fn run_validation_reports_dy_operation_oracle_gap_failures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000436.json"),
        r#"{
  "Core": { "Id": "CORE-000436", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Operations": [
    {
      "id": "$val_dy",
      "name": "--DTC",
      "operator": "dy"
    }
  ],
  "Check": {
    "all": [
      { "name": "--DY", "operator": "non_empty" },
      { "name": "--DTC", "operator": "is_complete_date" },
      { "name": "RFSTDTC", "operator": "is_complete_date" },
      { "name": "--DY", "operator": "not_equal_to", "value": "$val_dy" }
    ]
  },
  "Outcome": { "Message": "--DY has oracle-specific dy semantics" }
}"#,
    )
    .expect("write dy oracle gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "USUBJID": ["S1"],
        "EXSEQ": [1],
        "EXDTC": ["2024-01-01"],
        "EXDY": [0]
      }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "USUBJID": ["S1"],
        "RFSTDTC": ["2024-01-01"]
      }
    }
  ]
}"#,
    )
    .expect("write dy oracle gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_executes_domain_placeholder_column_ref_comparator() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DOMAIN-PLACEHOLDER-COLUMN-REF.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-PLACEHOLDER-COLUMN-REF", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--SCAT",
    "operator": "equal_to_case_insensitive",
    "value": "--DECOD"
  },
  "Outcome": {
    "Message": "--SCAT must match --DECOD",
    "Output Variables": ["--DECOD", "--SCAT"]
  }
}"#,
    )
    .expect("write domain placeholder comparator rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "AESEQ": [1, 2],
        "AEDECOD": ["HEADACHE", "NAUSEA"],
        "AESCAT": ["headache", "CARDIAC"]
      }
    }
  ]
}"#,
    )
    .expect("write domain placeholder comparator data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("1"));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["AEDECOD".to_owned(), "AESCAT".to_owned()]
    );
}

#[test]
fn run_validation_reports_domain_placeholder_column_ref_oracle_gap_failure() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000195.json"),
        r#"{
  "Core": { "Id": "CORE-000195", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "--SCAT",
    "operator": "equal_to_case_insensitive",
    "value": "--DECOD"
  },
  "Outcome": { "Message": "--SCAT repeats --DECOD" }
}"#,
    )
    .expect("write domain placeholder oracle gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "AEDECOD": ["HEADACHE"],
        "AESCAT": ["HEADACHE"]
      }
    }
  ]
}"#,
    )
    .expect("write domain placeholder oracle gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
}

#[test]
fn run_validation_executes_inner_join_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-INNER-JOIN.json"),
        r#"{
  "Core": { "Id": "CORE-INNER-JOIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "inner_join",
      "left": "AE",
      "right": "LOOKUP",
      "by": ["USUBJID"],
      "prefix": "LOOKUP."
    }
  ],
  "Check": {
    "name": "LOOKUP.FLAG",
    "operator": "equal_to",
    "value": "Y"
  },
  "Outcome": { "Message": "Matched lookup flag must not be Y" }
}"#,
    )
    .expect("write inner join rule");

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
        "AESEQ": [1, 2]
      }
    },
    {
      "filename": "lookup.json",
      "domain": "LOOKUP",
      "records": {
        "USUBJID": ["S2"],
        "FLAG": ["Y"]
      }
    }
  ]
}"#,
    )
    .expect("write inner join data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_executes_jsonata_string_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-JSONATA-STRING.json"),
        r#"{
  "Core": { "Id": "CORE-JSONATA-STRING", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "JSONATA",
  "Check": "$exists(DOMAIN) and DOMAIN != 'AE'",
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write jsonata string rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_planned_enrollment_jsonata_unit_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000981.json"),
            r#"{
  "Core": { "Id": "CORE-000981", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InPopQ:=$boolean(plannedEnrollmentNumber.unit); {\"check\": $InPopQ=true} )][check = true]",
  "Outcome": {
    "Message": "A unit has been specified for a planned enrollment number",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "plannedEnrollmentNumber": {
                "id": "Quantity_1",
                "value": 22,
                "unit": {
                  "id": "Unit_1",
                  "standardCode": { "decode": "Day", "code": "C25301" }
                }
              },
              "cohorts": []
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "StudyDesignPopulation"
    );
}

#[test]
fn run_validation_executes_usdm_planned_enrollment_cohort_consistency_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000963.json"),
            r#"{
  "Core": { "Id": "CORE-000963", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Check": "($.**.studyDesigns)@$sd.$sd.population@$p.$p[( $InCohort:=$boolean(cohorts.plannedEnrollmentNumber); $InPop:=($type(plannedEnrollmentNumber) != \"null\" and $exists(plannedEnrollmentNumber)); {\"check\": (($InPop=true and $InCohort=true) or ($InPop=false and $InCohort=true))} )][check=true]",
  "Outcome": {
    "Message": "A planned enrollment number has been specified for both the study population and the cohorts, or it has been specified for only a subset of the cohorts.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "plannedEnrollmentNumber.id",
      "plannedEnrollmentNumber(value/range)",
      "cohorts.name",
      "cohorts.plannedEnrollmentNumber.id",
      "cohorts.plannedEnrollmentNumber(value/range)"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Main Design",
            "population": {
              "id": "Population_1",
              "name": "POP1",
              "instanceType": "StudyDesignPopulation",
              "cohorts": [
                {
                  "id": "StudyCohort_1",
                  "name": "COHORT1",
                  "plannedEnrollmentNumber": { "id": "Quantity_1", "value": 10 }
                },
                { "id": "StudyCohort_2", "name": "COHORT2" }
              ]
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "StudyDesignPopulation"
    );
}

#[test]
fn run_validation_executes_usdm_sponsor_role_applies_to_version_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000974.json"),
            r#"{
  "Core": { "Id": "CORE-000974", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "study.versions@$sv.($sv.roles[code.code = \"C70793\" and $not($sv.id in appliesToIds)])@$r.{\"check\": true}",
  "Outcome": {
    "Message": "The study role is a sponsor role (code.code is C70793) but it is not applicable to the study version.",
    "Output Variables": ["name", "code.code", "code.decode", "appliesToIds", "StudyVersion.id"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "ROLE_1",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": []
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyRole");
}

#[test]
fn run_validation_executes_usdm_main_timeline_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000407.json"),
        r##"{
  "Core": { "Id": "CORE-000407", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Main timelines` != 1][]",
  "Outcome": {
    "Message": "The study design does not have exactly one main timeline.",
    "Output Variables": ["name", "# Main timelines", "Main timelines"]
  }
}"##,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Study Design 1",
            "instanceType": "InterventionalStudyDesign",
            "scheduleTimelines": []
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
}

#[test]
fn run_validation_executes_usdm_timeline_order_consistency_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (rule_id, previous_next, timeline_refs) in [
        (
            "CORE-000961",
            "Encounter order by previous/next",
            "Encounter order by timeline refs",
        ),
        (
            "CORE-001048",
            "Epoch order by previous/next",
            "Epoch order by timeline refs",
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{rule_id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["StudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Timeline order is inconsistent.",
    "Output Variables": [
      "name",
      "ScheduleTimeline.id",
      "ScheduleTimeline.name",
      "ScheduleTimeline.mainTimeline",
      "{previous_next}",
      "{timeline_refs}"
    ]
  }}
}}"#
            ),
        )
        .expect("write rule");
    }

    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "epochs": [
              { "id": "Epoch_1", "name": "Screening", "nextId": "Epoch_2" },
              { "id": "Epoch_2", "name": "Treatment", "previousId": "Epoch_1", "nextId": "Epoch_3" },
              { "id": "Epoch_3", "name": "Follow-up", "previousId": "Epoch_2" }
            ],
            "encounters": [
              { "id": "Encounter_1", "name": "E1", "nextId": "Encounter_2" },
              { "id": "Encounter_2", "name": "E2", "previousId": "Encounter_1", "nextId": "Encounter_3" },
              { "id": "Encounter_3", "name": "E3", "previousId": "Encounter_2" }
            ],
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "name": "Main",
                "mainTimeline": true,
                "instances": [
                  { "id": "Instance_1", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_1", "encounterId": "Encounter_1" },
                  { "id": "Instance_2", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_3", "encounterId": "Encounter_3" },
                  { "id": "Instance_3", "instanceType": "ScheduledActivityInstance", "epochId": "Epoch_2", "encounterId": "Encounter_2" }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    for result in &outcome.results {
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.errors[0].dataset, "StudyDesign");
        assert_eq!(result.errors[0].row, Some(1));
    }
}

#[test]
fn run_validation_executes_usdm_governance_date_global_scope_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000968.json"),
            r#"{
  "Core": { "Id": "CORE-000968", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["GovernanceDate"] } },
  "Check": "study.documentedBy.versions.dateValues.{\"check\": true}",
  "Outcome": {
    "Message": "There is more than one date of this type for the study definition document version, but at least one of the dates has a global geographic scope.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "type",
      "dateValue",
      "geographicScopes.type"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "dateValues": [
              {
                "id": "GovernanceDate_1",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-01",
                "geographicScopes": [
                  { "id": "GeographicScope_1", "type": { "code": "C68846", "decode": "Global" } }
                ]
              },
              {
                "id": "GovernanceDate_2",
                "instanceType": "GovernanceDate",
                "type": { "code": "C71476", "decode": "Approval Date" },
                "dateValue": "2020-01-02",
                "geographicScopes": [
                  { "id": "GeographicScope_2", "type": { "code": "C41129", "decode": "Region" } }
                ]
              },
              {
                "id": "GovernanceDate_3",
                "instanceType": "GovernanceDate",
                "type": { "code": "C215663", "decode": "Effective Date" },
                "dateValue": "2020-01-03",
                "geographicScopes": [
                  { "id": "GeographicScope_3", "type": { "code": "C41129", "decode": "Region" } }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "GovernanceDate");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_document_content_reference_one_to_one_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000985.json"),
            r#"{
  "Core": { "Id": "CORE-000985", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["DocumentContentReference"] } },
  "Check": "study.versions.amendments.changes.changedSections.{\"check\": true}",
  "Outcome": {
    "Message": "There is not a one-to-one relationship between the referenced section number and title within the study definition document affected by the study amendment.",
    "Output Variables": [
      "StudyAmendment.id",
      "StudyAmendment.name",
      "StudyChange.id",
      "StudyChange.name",
      "appliesToId",
      "sectionNumber",
      "sectionTitle"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      { "id": "StudyDefinitionDocument_1", "name": "Protocol" }
    ],
    "versions": [
      {
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "changes": [
              {
                "id": "StudyChange_1",
                "name": "Change 1",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_1",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "1",
                    "sectionTitle": "Intro"
                  },
                  {
                    "id": "DocumentContentReference_2",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "2",
                    "sectionTitle": "Intro"
                  }
                ]
              },
              {
                "id": "StudyChange_2",
                "name": "Change 2",
                "changedSections": [
                  {
                    "id": "DocumentContentReference_3",
                    "instanceType": "DocumentContentReference",
                    "appliesToId": "StudyDefinitionDocument_1",
                    "sectionNumber": "3",
                    "sectionTitle": "Methods"
                  }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "DocumentContentReference"
    );
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_json_schema_check_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000935.json"),
        r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": 71476, "decode": "Approval Date" }
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "population": {
              "plannedAge": {
                "isApproximate": "false"
              }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "JSONSchemaIssue");
}

#[test]
fn run_validation_executes_usdm_json_schema_check_rules_with_no_issues() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000935.json"),
        r#"{
  "Core": { "Id": "CORE-000935", "Status": "Published" },
  "Rule Type": "JSON Schema Check",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": { "name": "validator", "operator": "equal_to", "value": "type" },
  "Outcome": {
    "Message": "The datatype of the attribute does not conform with the USDM schema.",
    "Output Variables": ["error_attribute", "message"]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dateValues": [
          {
            "id": "GovernanceDate_1",
            "type": { "code": "C71476", "decode": "Approval Date" },
            "geographicScopes": [
              { "id": "GeographicScope_1", "code": null }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_executes_usdm_primary_endpoint_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001036.json"),
            r##"{
  "Core": { "Id": "CORE-001036", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}[`# Primary endpoints` = 0][]",
  "Outcome": {
    "Message": "There is not at least one endpoint with a level of primary within the study design.",
    "Output Variables": ["name", "# Primary endpoints"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design without primary endpoint",
            "instanceType": "InterventionalStudyDesign",
            "objectives": [
              {
                "id": "Objective_1",
                "endpoints": [
                  { "id": "Endpoint_1", "level": { "code": "C98772", "decode": "Secondary" } }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyDesign");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "# Primary endpoints"]
    );
}

#[test]
fn run_validation_executes_usdm_interventional_model_intervention_count_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001077.json"),
            r##"{
  "Core": { "Id": "CORE-001077", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The number of study interventions referenced for the interventional study design is not consistent with intervention model.",
    "Output Variables": ["name", "studyType.code", "studyType.decode", "model.code", "model.decode", "# Referenced Study Interventions"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyInterventions": [
          { "id": "StudyIntervention_1", "instanceType": "StudyIntervention" },
          { "id": "StudyIntervention_2", "instanceType": "StudyIntervention" }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Too few interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Enough interventions",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82639", "decode": "Parallel Study" },
            "studyInterventionIds": ["StudyIntervention_1", "StudyIntervention_2"]
          },
          {
            "id": "StudyDesign_3",
            "name": "Single group",
            "instanceType": "StudyDesign",
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "model": { "code": "C82640", "decode": "Single Group" },
            "studyInterventionIds": ["StudyIntervention_1"]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "InterventionalStudyDesign"
    );
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "name",
            "studyType.code",
            "studyType.decode",
            "model.code",
            "model.decode",
            "# Referenced Study Interventions"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_duplicate_study_cell_arm_epoch_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000948.json"),
            r#"{
  "Core": { "Id": "CORE-000948", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyCell"] } },
  "Check": "$.study.versions.studyDesigns.studyCells.{\"check\": true}",
  "Outcome": {
    "Message": "The combination of arm and epoch occurs more than once within the study design.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "armId", "StudyArm.name", "epochId", "StudyEpoch.name"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "instanceType": "StudyCell", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyCell");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDesign.id",
            "StudyDesign.name",
            "armId",
            "StudyArm.name",
            "epochId",
            "StudyEpoch.name"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_study_arm_missing_epoch_refs_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001026.json"),
        r#"{
  "Core": { "Id": "CORE-001026", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyArm"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.arms@$sa.{\"check\": true}",
  "Outcome": {
    "Message": "The StudyArm does not have one StudyCell for each StudyEpoch.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "StudyDesign.epochs",
      "Arm's StudyCell Epoch Refs",
      "Missing Epoch Refs"
    ]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "arms": [
              { "id": "StudyArm_1", "name": "Arm A", "instanceType": "StudyArm" },
              { "id": "StudyArm_2", "name": "Arm B", "instanceType": "StudyArm" }
            ],
            "epochs": [
              { "id": "StudyEpoch_1", "name": "Screening" },
              { "id": "StudyEpoch_2", "name": "Treatment" }
            ],
            "studyCells": [
              { "id": "StudyCell_1", "armId": "StudyArm_1", "epochId": "StudyEpoch_1" },
              { "id": "StudyCell_2", "armId": "StudyArm_1", "epochId": "StudyEpoch_2" },
              { "id": "StudyCell_3", "armId": "StudyArm_2", "epochId": "StudyEpoch_1" }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyArm");
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_condition_applies_to_reference_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-001038.json"),
        r#"{
  "Core": { "Id": "CORE-001038", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition"] } },
  "Check": "$.study.versions.**.conditions.appliesToIds.{\"check\": true}",
  "Outcome": {
    "Message": "Condition appliesToIds must reference an allowed instance type.",
    "Output Variables": ["name", "appliesTo id", "appliesTo instanceType"]
  }
}"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "activities": [
          { "id": "Activity_1", "name": "Dose", "instanceType": "Activity" }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Bad condition",
            "instanceType": "Condition",
            "appliesToIds": ["Activity_1", "Missing_1", "Condition_1"]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "Condition");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "appliesTo id", "appliesTo instanceType"]
    );
}

#[test]
fn run_validation_executes_usdm_parameter_map_reference_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001049.json"),
            r#"{
  "Core": { "Id": "CORE-001049", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ParameterMap"] } },
  "Check": "$.study.**.dictionaries.parameterMaps.{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the parameter map is not available elsewhere in the model.",
    "Output Variables": ["SyntaxTemplateDictionary.id", "SyntaxTemplateDictionary.name", "tag", "reference"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "activities": [
          { "id": "Activity_1", "name": "Dose", "label": "Dose activity", "instanceType": "Activity" }
        ],
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "Dictionary",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              {
                "id": "ParameterMap_1",
                "instanceType": "ParameterMap",
                "tag": "valid_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_1\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_2",
                "instanceType": "ParameterMap",
                "tag": "missing_activity",
                "reference": "<usdm:ref klass=\"Activity\" id=\"Activity_xx\" attribute=\"label\"></usdm:ref>"
              },
              {
                "id": "ParameterMap_3",
                "instanceType": "ParameterMap",
                "tag": "partial_ref",
                "reference": "<usdm:ref attribute=\"label\" id=\"Activity_1\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "ParameterMap");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "SyntaxTemplateDictionary.id",
            "SyntaxTemplateDictionary.name",
            "tag",
            "reference"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_blinding_schema_masked_roles_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001072.json"),
            r##"{
  "Core": { "Id": "CORE-001072", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a blinding schema that is not open label or double blind but there is no applicable study role that is masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1"]
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "No masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Has masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C28233", "decode": "SINGLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_3",
            "name": "Open label",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "InterventionalStudyDesign"
    );
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "name",
            "blindingSchema.code",
            "blindingSchema.decode",
            "# Masked Roles",
            "Applicable Roles"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_double_blind_requires_two_masked_roles_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001071.json"),
            r##"{
  "Core": { "Id": "CORE-001071", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns.{\"check\": true}",
  "Outcome": {
    "Message": "The study design has a double blind blinding schema but there are not at least two applicable study roles that are masked.",
    "Output Variables": ["name", "blindingSchema.code", "blindingSchema.decode", "# Masked Roles", "Applicable Roles"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          },
          {
            "id": "StudyRole_3",
            "instanceType": "StudyRole",
            "code": { "decode": "Assessor" },
            "appliesToIds": ["InterventionalStudyDesign_2"],
            "masking": { "isMasked": true }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Only one masked role",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
            }
          },
          {
            "id": "InterventionalStudyDesign_2",
            "name": "Two masked roles",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C15228", "decode": "DOUBLE BLIND" }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].dataset,
        "InterventionalStudyDesign"
    );
}

#[test]
fn run_validation_executes_usdm_open_label_rejects_masked_role_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001070.json"),
            r##"{
  "Core": { "Id": "CORE-001070", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles.{\"check\": true}",
  "Outcome": {
    "Message": "A masking is defined for the study role, but the role applies to a study design with an open label blinding schema.",
    "Output Variables": ["name", "code", "masking.text", "masking.isMasked", "appliesToIds", "StudyDesign.id", "StudyDesign.blindingSchema"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Masked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Investigator" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Masked", "isMasked": true }
          },
          {
            "id": "StudyRole_2",
            "name": "Unmasked open-label role",
            "instanceType": "StudyRole",
            "code": { "decode": "Study Subject" },
            "appliesToIds": ["InterventionalStudyDesign_1"],
            "masking": { "text": "Not masked", "isMasked": false }
          }
        ],
        "studyDesigns": [
          {
            "id": "InterventionalStudyDesign_1",
            "name": "Open label design",
            "instanceType": "InterventionalStudyDesign",
            "blindingSchema": {
              "standardCode": { "code": "C49659", "decode": "OPEN LABEL" }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyRoleBlinding");
}

#[test]
fn run_validation_executes_usdm_abbreviation_expanded_text_duplicate_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001067.json"),
            r##"{
  "Core": { "Id": "CORE-001067", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's expanded text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "Cu",
            "expandedText": "copper"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "LBC",
            "expandedText": "Copper"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyVersion.id",
            "StudyVersion.versionIdentifier",
            "abbreviatedText",
            "expandedText"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_abbreviation_text_duplicate_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001053.json"),
            r##"{
  "Core": { "Id": "CORE-001053", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Abbreviation"] } },
  "Check": "$.study.versions.abbreviations.{\"check\": true}",
  "Outcome": {
    "Message": "The abbreviation's abbreviated text is not unique within the study version.",
    "Output Variables": ["StudyVersion.id", "StudyVersion.versionIdentifier", "abbreviatedText", "expandedText"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "abbreviations": [
          {
            "id": "Abbreviation_1",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse event"
          },
          {
            "id": "Abbreviation_2",
            "instanceType": "Abbreviation",
            "abbreviatedText": "AE",
            "expandedText": "adverse experience"
          },
          {
            "id": "Abbreviation_3",
            "instanceType": "Abbreviation",
            "abbreviatedText": "BMI",
            "expandedText": "body mass index"
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "Abbreviation");
}

#[test]
fn run_validation_executes_usdm_duplicate_document_version_ids_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001052.json"),
            r##"{
  "Core": { "Id": "CORE-001052", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Check": "$.study.versions.{\"check\": true}",
  "Outcome": {
    "Message": "The study version references the same study definition document version more than once.",
    "Output Variables": ["versionIdentifier", "Duplicate documentVersionIds"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "2",
        "documentVersionIds": ["DocVersion_1", "DocVersion_2", "DocVersion_1"]
      },
      {
        "id": "StudyVersion_2",
        "versionIdentifier": "3",
        "documentVersionIds": ["DocVersion_1", "DocVersion_2"]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyVersion");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["versionIdentifier", "Duplicate documentVersionIds"]
    );
}

#[test]
fn run_validation_executes_usdm_tag_parameter_dictionary_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001074.json"),
            r##"{
  "Core": { "Id": "CORE-001074", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
            rules_dir.join("CORE-001037.json"),
            r##"{
  "Core": { "Id": "CORE-001037", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Condition", "Endpoint"] } },
  "Check": "$.study.versions.**[$contains(text,/usdm:tag/)].{\"check\": true}",
  "Outcome": {
    "Message": "The parameter name referenced in the text is not specified in the data dictionary parameter map.",
    "Output Variables": ["name", "Parameter reference", "Parameter name", "dictionaryId", "SyntaxTemplateDictionary.name", "Issue"]
  }
}"##,
        )
        .expect("write CORE-001037 rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "dictionaries": [
          {
            "id": "SyntaxTemplateDictionary_1",
            "name": "IE_Dict",
            "instanceType": "SyntaxTemplateDictionary",
            "parameterMaps": [
              { "id": "ParameterMap_1", "instanceType": "ParameterMap", "tag": "valid_tag" }
            ]
          }
        ],
        "conditions": [
          {
            "id": "Condition_1",
            "name": "Missing dictionary",
            "instanceType": "Condition",
            "text": "Use <usdm:tag name=\"missing_dict\"/>"
          },
          {
            "id": "Condition_2",
            "name": "Invalid dictionary",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_xx",
            "text": "Use <usdm:tag name=\"bad_dict\"/>"
          },
          {
            "id": "Condition_3",
            "name": "Missing tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"not_in_dictionary\"></usdm:tag>"
          },
          {
            "id": "Condition_4",
            "name": "Valid tag",
            "instanceType": "Condition",
            "dictionaryId": "SyntaxTemplateDictionary_1",
            "text": "Use <usdm:tag name=\"valid_tag\"/>"
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    for id in ["CORE-001037", "CORE-001074"] {
        let result = outcome
            .results
            .iter()
            .find(|result| result.rule_id == id)
            .expect("result by id");
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 3);
        assert_eq!(result.errors[0].dataset, "SyntaxTemplateText");
        assert_eq!(
            result.errors[0].variables,
            vec![
                "name",
                "Parameter reference",
                "Parameter name",
                "dictionaryId",
                "SyntaxTemplateDictionary.name",
                "Issue"
            ]
        );
    }
}

#[test]
fn run_validation_executes_usdm_narrative_content_ref_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001073.json"),
            r##"{
  "Core": { "Id": "CORE-001073", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContentItem"] } },
  "Check": "$.study.versions.narrativeContentItems[$contains(text,/usdm:ref/)].{\"check\": true}",
  "Outcome": {
    "Message": "The item referenced in the narrative content item text is not available elsewhere in the model.",
    "Output Variables": ["name", "Invalid Reference"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyIdentifiers": [
          {
            "id": "StudyIdentifier_1",
            "name": "NCT identifier",
            "instanceType": "StudyIdentifier",
            "text": "NCT-001"
          }
        ],
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Missing klass",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_2",
            "name": "Missing target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_xx\" klass=\"StudyIdentifier\"></usdm:ref>"
          },
          {
            "id": "NarrativeContentItem_3",
            "name": "Valid target",
            "instanceType": "NarrativeContentItem",
            "text": "See <usdm:ref attribute=\"text\" id=\"StudyIdentifier_1\" klass=\"StudyIdentifier\"></usdm:ref>"
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContentItem");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "Invalid Reference"]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_item_id_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000944.json"),
            r##"{
  "Core": { "Id": "CORE-000944", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[contentItemId and $not(contentItemId in $.study.versions.narrativeContentItems.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The reference to the narrative content item is not targeting a narrative content item that has been defined within the study.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "contentItemId",
      "sectionNumber"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "narrativeContentItems": [
          {
            "id": "NarrativeContentItem_1",
            "name": "Defined",
            "instanceType": "NarrativeContentItem"
          }
        ]
      }
    ],
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Valid",
                "instanceType": "NarrativeContent",
                "contentItemId": "NarrativeContentItem_1",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Missing A",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_A",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Missing B",
                "instanceType": "NarrativeContent",
                "contentItemId": "Missing_B",
                "sectionNumber": "3"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "contentItemId",
            "sectionNumber"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_peer_refs_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001055.json"),
            r##"{
  "Core": { "Id": "CORE-001055", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[previousId or nextId or childIds].{\"check\": true}",
  "Outcome": {
    "Message": "The narrative content references a previous, next or child id value that does not match the id of any narrative content defined within the same study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "Invalid previousId",
      "Invalid nextId",
      "Invalid childIds"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "instanceType": "StudyDefinitionDocument",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Bad next",
                "instanceType": "NarrativeContent",
                "nextId": "Missing_Next",
                "sectionNumber": "1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Good",
                "instanceType": "NarrativeContent",
                "previousId": "NarrativeContent_1",
                "nextId": "NarrativeContent_3",
                "sectionNumber": "2"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Bad previous and child",
                "instanceType": "NarrativeContent",
                "previousId": "Missing_Previous",
                "childIds": ["NarrativeContent_2", "Missing_Child"],
                "sectionNumber": "3"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "sectionNumber",
            "Invalid previousId",
            "Invalid nextId",
            "Invalid childIds"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_display_section_number_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000964.json"),
            r##"{
  "Core": { "Id": "CORE-000964", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionNumber=true and (sectionNumber=null or sectionNumber=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section number is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionNumber",
      "sectionNumber"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present number",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "displaySectionNumber",
            "sectionNumber"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_narrative_content_duplicate_section_number_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001041.json"),
            r#"{
  "Core": { "Id": "CORE-001041", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy@$sdd.$sdd.versions@$sddv.($sddv.contents[displaySectionNumber=true and sectionNumber].{\"check\": true})",
  "Outcome": {
    "Message": "The displayed section number is not unique within the study definition document version.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "sectionNumber",
      "displaySectionNumber"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Duplicate A",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Duplicate B",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden duplicate",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": false,
                "sectionNumber": "1.1"
              },
              {
                "id": "NarrativeContent_4",
                "name": "Unique",
                "instanceType": "NarrativeContent",
                "displaySectionNumber": true,
                "sectionNumber": "2.1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "NarrativeContent");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_narrative_content_display_section_title_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000965.json"),
            r##"{
  "Core": { "Id": "CORE-000965", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["NarrativeContent"] } },
  "Check": "$.study.documentedBy.versions.contents[displaySectionTitle=true and (sectionTitle=null or sectionTitle=\"\")].{\"check\": true}",
  "Outcome": {
    "Message": "A section title is indicated to be displayed but not specified.",
    "Output Variables": [
      "StudyDefinitionDocument.id",
      "StudyDefinitionDocument.name",
      "StudyDefinitionDocumentVersion.id",
      "StudyDefinitionDocumentVersion.version",
      "name",
      "displaySectionTitle",
      "sectionTitle"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1.0",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Missing title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true
              },
              {
                "id": "NarrativeContent_2",
                "name": "Blank title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": ""
              },
              {
                "id": "NarrativeContent_3",
                "name": "Hidden title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": false
              },
              {
                "id": "NarrativeContent_4",
                "name": "Present title",
                "instanceType": "NarrativeContent",
                "displaySectionTitle": true,
                "sectionTitle": "Introduction"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDefinitionDocument.id",
            "StudyDefinitionDocument.name",
            "StudyDefinitionDocumentVersion.id",
            "StudyDefinitionDocumentVersion.version",
            "name",
            "displaySectionTitle",
            "sectionTitle"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_activity_child_id_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001062.json"),
            r##"{
  "Core": { "Id": "CORE-001062", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities.childIds[$not($ in $.study.versions.studyDesigns.activities.id)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity references a childId that does not match the id of any activity defined within the same study design as the activity.",
    "Output Variables": ["StudyDesign.id", "StudyDesign.name", "name", "childId"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "instanceType": "InterventionalStudyDesign",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Missing_A", "Missing_B"]
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "childIds": []
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["StudyDesign.id", "StudyDesign.name", "name", "childId"]
    );
}

#[test]
fn run_validation_executes_usdm_activity_children_with_details_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000954.json"),
            r##"{
  "Core": { "Id": "CORE-000954", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions.studyDesigns.activities[childIds and (biomedicalConceptIds or bcCategoryIds or definedProcedures or timelineId or bcSurrogateIds)].{\"check\": true}",
  "Outcome": {
    "Message": "The activity has children but also refers to details.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "childIds",
      "biomedicalConceptIds",
      "bcCategoryIds",
      "bcSurrogateIds",
      "timelineId",
      "definedProcedures.id"
    ]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent with timeline",
                "instanceType": "Activity",
                "childIds": ["Activity_2", "Activity_3"],
                "timelineId": "Timeline_1"
              },
              {
                "id": "Activity_2",
                "name": "Parent with BC",
                "instanceType": "Activity",
                "childIds": ["Activity_3"],
                "biomedicalConceptIds": ["BC_1"]
              },
              {
                "id": "Activity_3",
                "name": "Leaf with details",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BC_2"]
              },
              {
                "id": "Activity_4",
                "name": "Parent only",
                "instanceType": "Activity",
                "childIds": ["Activity_3"]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "StudyDesign.id",
            "StudyDesign.name",
            "name",
            "childIds",
            "biomedicalConceptIds",
            "bcCategoryIds",
            "bcSurrogateIds",
            "timelineId",
            "definedProcedures.id"
        ]
    );
}

#[test]
fn run_validation_executes_usdm_activity_child_order_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001066.json"),
            r#"{
  "Core": { "Id": "CORE-001066", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "study.versions.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The previous/next ordering of the activity with respect to child activities is incorrect.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "previousId",
      "nextId",
      "childIds",
      "Parent Activity's id"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Parent",
                "instanceType": "Activity",
                "childIds": ["Activity_2"],
                "nextId": "Activity_3"
              },
              {
                "id": "Activity_2",
                "name": "Child",
                "instanceType": "Activity",
                "previousId": "Activity_1"
              },
              {
                "id": "Activity_3",
                "name": "Other",
                "instanceType": "Activity",
                "previousId": "Activity_2"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_usdm_activity_bc_category_overlap_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001047.json"),
            r#"{
  "Core": { "Id": "CORE-001047", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Activity"] } },
  "Check": "$.study.versions@$sv.$sv.studyDesigns@$sd.$sd.activities@$a.{\"check\": true}",
  "Outcome": {
    "Message": "The activity references both a biomedical concept category and a biomedical concept, but the biomedical concept is a member of the referenced category or one of its subcategories.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "biomedicalConceptId",
      "bcCategoryId(s) containing BC"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "bcCategories": [
          {
            "id": "BCCategory_1",
            "name": "Vitals",
            "memberIds": ["BiomedicalConcept_1"]
          },
          {
            "id": "BCCategory_2",
            "name": "Labs",
            "memberIds": ["BiomedicalConcept_2"]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "activities": [
              {
                "id": "Activity_1",
                "name": "Overlapping Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_1"]
              },
              {
                "id": "Activity_2",
                "name": "Non-overlap Activity",
                "instanceType": "Activity",
                "biomedicalConceptIds": ["BiomedicalConcept_1"],
                "bcCategoryIds": ["BCCategory_2"]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "Activity");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_usdm_scheduled_instance_design_reference_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (rule_id, field, parent_field) in [
        (
            "CORE-000950",
            "epochId",
            "Referenced epoch's parent StudyDesign.id",
        ),
        (
            "CORE-001039",
            "encounterId",
            "Referenced encounter's parent StudyDesign.id",
        ),
    ] {
        fs::write(
            rules_dir.join(format!("{rule_id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["ScheduledActivityInstance"] }} }},
  "Check": "$.study.versions.studyDesigns.scheduleTimelines.instances.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Scheduled instance references an object outside the design.",
    "Output Variables": [
      "StudyDesign.id",
      "StudyDesign.name",
      "name",
      "{field}",
      "{parent_field}"
    ]
  }}
}}"#
            ),
        )
        .expect("write rule");
    }

    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design 1",
            "epochs": [{ "id": "StudyEpoch_1", "name": "Epoch 1" }],
            "encounters": [{ "id": "Encounter_1", "name": "Encounter 1" }],
            "scheduleTimelines": []
          },
          {
            "id": "StudyDesign_2",
            "name": "Design 2",
            "epochs": [{ "id": "StudyEpoch_2", "name": "Epoch 2" }],
            "encounters": [{ "id": "Encounter_2", "name": "Encounter 2" }],
            "scheduleTimelines": [
              {
                "id": "ScheduleTimeline_1",
                "instances": [
                  {
                    "id": "ScheduledActivityInstance_1",
                    "name": "Bad epoch",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_1",
                    "encounterId": "Encounter_2"
                  },
                  {
                    "id": "ScheduledActivityInstance_2",
                    "name": "Bad encounter",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_1"
                  },
                  {
                    "id": "ScheduledActivityInstance_3",
                    "name": "Good refs",
                    "instanceType": "ScheduledActivityInstance",
                    "epochId": "StudyEpoch_2",
                    "encounterId": "Encounter_2"
                  }
                ]
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let epoch_result = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-000950")
        .expect("epoch result");
    let encounter_result = outcome
        .results
        .iter()
        .find(|result| result.rule_id == "CORE-001039")
        .expect("encounter result");
    assert_eq!(epoch_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(epoch_result.error_count, 1);
    assert_eq!(epoch_result.errors[0].row, Some(1));
    assert_eq!(encounter_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(encounter_result.error_count, 1);
    assert_eq!(encounter_result.errors[0].row, Some(2));
}

#[test]
fn run_validation_executes_usdm_study_role_assigned_persons_and_orgs_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000997.json"),
        r##"{
  "Core": { "Id": "CORE-000997", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyRole"] } },
  "Check": "$.study.versions.roles[assignedPersons and organizationIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study role references both assigned persons and organizations.",
    "Output Variables": ["name", "code", "assignedPersons", "organizationIds"]
  }
}"##,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Sponsor",
            "instanceType": "Organization"
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "name": "Person only",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_1", "name": "AP1" }
            ]
          },
          {
            "id": "StudyRole_2",
            "name": "Org only",
            "instanceType": "StudyRole",
            "code": { "code": "C215670", "decode": "Local Sponsor" },
            "organizationIds": ["Organization_1"]
          },
          {
            "id": "StudyRole_3",
            "name": "Both",
            "instanceType": "StudyRole",
            "code": { "code": "C25936", "decode": "Investigator" },
            "assignedPersons": [
              { "id": "AssignedPerson_2", "name": "AP2" }
            ],
            "organizationIds": ["Organization_1"]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "code", "assignedPersons", "organizationIds"]
    );
}

#[test]
fn run_validation_executes_usdm_duration_quantity_text_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000994.json"),
        r##"{
  "Core": { "Id": "CORE-000994", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and not(text) and not(quantity)].{\"check\": true}",
  "Outcome": {
    "Message": "The quantity and text are both missing.",
    "Output Variables": ["text", "quantity"]
  }
}"##,
    )
    .expect("write rule");
    fs::write(
            rules_dir.join("CORE-000995.json"),
            r##"{
  "Core": { "Id": "CORE-000995", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["Duration"] } },
  "Check": "$.**[instanceType=\"Duration\" and ((durationWillVary=true and quantity) or (durationWillVary=false and not(quantity)))].{\"check\": true}",
  "Outcome": {
    "Message": "The duration quantity conflicts with durationWillVary.",
    "Output Variables": ["quantity(value/range)", "durationWillVary"]
  }
}"##,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "scheduleTimelines": [
              {
                "id": "Timeline_1",
                "plannedDuration": {
                  "id": "Duration_1",
                  "instanceType": "Duration",
                  "durationWillVary": false
                }
              }
            ]
          }
        ],
        "studyInterventions": [
          {
            "id": "Intervention_1",
            "administrations": [
              {
                "id": "Administration_1",
                "duration": {
                  "id": "Duration_2",
                  "instanceType": "Duration",
                  "durationWillVary": true,
                  "quantity": {
                    "value": 24,
                    "unit": {
                      "standardCode": {
                        "decode": "Week",
                        "code": "C29844"
                      }
                    }
                  }
                }
              },
              {
                "id": "Administration_2",
                "duration": {
                  "id": "Duration_3",
                  "instanceType": "Duration",
                  "text": "Variable"
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let missing = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000994.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run missing");
    assert_eq!(missing.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(missing.results[0].error_count, 1);
    assert_eq!(
        missing.results[0].errors[0].variables,
        vec!["text", "quantity"]
    );

    let conflict = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000995.json")],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run conflict");
    assert_eq!(
        conflict.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(conflict.results[0].error_count, 2);
    assert_eq!(
        conflict.results[0].errors[0].variables,
        vec!["quantity(value/range)", "durationWillVary"]
    );
}

#[test]
fn run_validation_executes_usdm_study_design_document_type_phase_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000998.json"),
        r##"{
  "Core": { "Id": "CORE-000998", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[documentVersionIds].{\"check\": true}",
  "Outcome": {
    "Message": "The study design references the same document version more than once.",
    "Output Variables": ["name", "Duplicate documentVersionIds"]
  }
}"##,
    )
    .expect("write duplicate rule");
    fs::write(
            rules_dir.join("CORE-001004.json"),
            r##"{
  "Core": { "Id": "CORE-001004", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and instanceType != \"ObservationalStudyDesign\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational study must use ObservationalStudyDesign.",
    "Output Variables": ["name", "studyType"]
  }
}"##,
        )
        .expect("write type rule");
    fs::write(
            rules_dir.join("CORE-001005.json"),
            r##"{
  "Core": { "Id": "CORE-001005", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[studyType.code in [\"C16084\",\"C129000\"] and studyPhase.standardCode.code != \"C48660\"].{\"check\": true}",
  "Outcome": {
    "Message": "Observational phase must be Not Applicable.",
    "Output Variables": ["name", "studyType", "studyPhase"]
  }
}"##,
        )
        .expect("write phase rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Duplicate doc",
            "instanceType": "InterventionalStudyDesign",
            "documentVersionIds": ["DocV_1", "DocV_1"]
          },
          {
            "id": "StudyDesign_2",
            "name": "Wrong class",
            "instanceType": "InterventionalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            }
          },
          {
            "id": "StudyDesign_3",
            "name": "Wrong phase",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C129000",
              "decode": "Patient Registry Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C15602",
                "decode": "Phase III Trial"
              }
            }
          },
          {
            "id": "StudyDesign_4",
            "name": "Valid observational",
            "instanceType": "ObservationalStudyDesign",
            "studyType": {
              "code": "C16084",
              "decode": "Observational Study"
            },
            "studyPhase": {
              "standardCode": {
                "code": "C48660",
                "decode": "Not Applicable"
              }
            }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let duplicate = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-000998.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run duplicate");
    assert_eq!(
        duplicate.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(duplicate.results[0].error_count, 1);
    assert_eq!(
        duplicate.results[0].errors[0].variables,
        vec!["name", "Duplicate documentVersionIds"]
    );

    let type_rule = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-001004.json")],
        dataset_paths: vec![data_dir.clone()],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run type");
    assert_eq!(
        type_rule.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(type_rule.results[0].error_count, 1);
    assert_eq!(
        type_rule.results[0].errors[0].variables,
        vec!["name", "studyType"]
    );

    let phase = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.join("CORE-001005.json")],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run phase");
    assert_eq!(phase.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(phase.results[0].error_count, 1);
    assert_eq!(
        phase.results[0].errors[0].variables,
        vec!["name", "studyType", "studyPhase"]
    );
}

#[test]
fn run_validation_executes_usdm_study_design_duplicate_code_list_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            ("CORE-000980", "[\"name\", \"characteristics\"]"),
            ("CORE-001002", "[\"name\", \"subTypes\"]"),
            (
                "CORE-001003",
                "[\"name\", \"therapeuticAreas.codeSystem\", \"therapeuticAreas.codeSystemVersion\", \"therapeuticAreas\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] }} }},
  "Check": "$.study.versions.studyDesigns.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Duplicate study design list values.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C1", "decode": "A" },
              { "id": "Code_2", "code": "C1", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_3", "code": "S1", "decode": "Sub A" },
              { "id": "Code_4", "code": "S1", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_5", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA A" },
              { "id": "Code_6", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T1", "decode": "TA B" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Valid",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_7", "code": "C2", "decode": "A" },
              { "id": "Code_8", "code": "C3", "decode": "B" }
            ],
            "subTypes": [
              { "id": "Code_9", "code": "S2", "decode": "Sub A" },
              { "id": "Code_10", "code": "S3", "decode": "Sub B" }
            ],
            "therapeuticAreas": [
              { "id": "Code_11", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T2", "decode": "TA A" },
              { "id": "Code_12", "codeSystem": "SYS", "codeSystemVersion": "1", "code": "T3", "decode": "TA B" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    for (id, variables) in [
        ("CORE-000980", vec!["name", "characteristics"]),
        ("CORE-001002", vec!["name", "subTypes"]),
        (
            "CORE-001003",
            vec![
                "name",
                "therapeuticAreas.codeSystem",
                "therapeuticAreas.codeSystemVersion",
                "therapeuticAreas",
            ],
        ),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run duplicate list");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_study_design_single_and_multi_centre_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001017.json"),
            r#"{
  "Core": { "Id": "CORE-001017", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["InterventionalStudyDesign", "ObservationalStudyDesign"] } },
  "Check": "$.study.versions.studyDesigns[\"C217004\" in characteristics.code and \"C217005\" in characteristics.code].{\"check\": true}",
  "Outcome": {
    "Message": "A study design must not be both single-centre and multicentre.",
    "Output Variables": ["name", "characteristics"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Conflicting",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_1", "code": "C217004", "decode": "Single-Centre" },
              { "id": "Code_2", "code": "C217005", "decode": "Multicentre" }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Single only",
            "instanceType": "InterventionalStudyDesign",
            "characteristics": [
              { "id": "Code_3", "code": "C217004", "decode": "Single-Centre" }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["name", "characteristics"]
    );
}

#[test]
fn run_validation_executes_usdm_range_and_person_name_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
        ("CORE-001009", "Range", "[\"minValue\", \"maxValue\"]"),
        ("CORE-001012", "Range", "[\"minValue\", \"maxValue\"]"),
        ("CORE-001014", "PersonName", "[\"familyName\", \"text\"]"),
    ] {
        fs::write(
            rules_dir.join(format!("{id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM recursive entity rule.",
    "Output Variables": {output}
  }}
}}"#
            ),
        )
        .expect("write rule");
    }

    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "population": {
              "plannedAge": {
                "instanceType": "Range",
                "minValue": {
                  "value": 50,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                },
                "maxValue": {
                  "value": 20,
                  "unit": { "standardCode": { "decode": "Year", "code": "C29848" } }
                }
              },
              "plannedCompletionNumber": {
                "instanceType": "Range",
                "minValue": { "value": 50 },
                "maxValue": {
                  "value": 100,
                  "unit": { "standardCode": { "decode": "Participant", "code": "C142710" } }
                }
              }
            }
          }
        ],
        "roles": [
          {
            "id": "StudyRole_1",
            "assignedPersons": [
              {
                "id": "AssignedPerson_1",
                "personName": {
                  "instanceType": "PersonName"
                }
              },
              {
                "id": "AssignedPerson_2",
                "personName": {
                  "instanceType": "PersonName",
                  "familyName": "Smith"
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    for (id, count, variables) in [
        ("CORE-001009", 1, vec!["minValue", "maxValue"]),
        ("CORE-001012", 1, vec!["minValue", "maxValue"]),
        ("CORE-001014", 1, vec!["familyName", "text"]),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run recursive entity");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, count);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_simple_recursive_entity_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
            (
                "CORE-000971",
                "Address",
                "[\"Organization.id\", \"Organization.name\", \"text\", \"lines\", \"district\", \"city\", \"postalCode\", \"state\", \"country\"]",
            ),
            (
                "CORE-001011",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\"]",
            ),
            (
                "CORE-001021",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\"]",
            ),
            (
                "CORE-001006",
                "BiomedicalConcept",
                "[\"name\", \"label/synonym\", \"synonyms\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM simple recursive rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          {
            "id": "Organization_1",
            "name": "Org",
            "legalAddress": {
              "id": "Address_1",
              "instanceType": "Address"
            }
          }
        ],
        "amendments": [
          {
            "id": "StudyAmendment_1",
            "name": "Amendment",
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C48660", "decode": "Not Applicable" }
            }
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "Role_1",
            "name": "Manufacturer",
            "instanceType": "ProductOrganizationRole"
          }
        ],
        "biomedicalConcepts": [
          {
            "id": "BC_1",
            "name": "Sex",
            "label": "Sex",
            "instanceType": "BiomedicalConcept",
            "synonyms": ["Gender", "sex"]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    for (id, variables) in [
        (
            "CORE-000971",
            vec![
                "Organization.id",
                "Organization.name",
                "text",
                "lines",
                "district",
                "city",
                "postalCode",
                "state",
                "country",
            ],
        ),
        (
            "CORE-001011",
            vec!["StudyAmendment.id", "StudyAmendment.name", "code"],
        ),
        ("CORE-001021", vec!["name", "appliesToIds"]),
        ("CORE-001006", vec!["name", "label/synonym", "synonyms"]),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run simple recursive");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].variables, variables);
    }
}

#[test]
fn run_validation_executes_usdm_administration_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            (
                "CORE-000966",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value)\", \"route.id\", \"route\"]",
            ),
            (
                "CORE-000967",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"frequency.id\", \"frequency\"]",
            ),
            (
                "CORE-000969",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"dose.id\", \"dose(value/range)\", \"administrableProductId\", \"medicalDeviceId\", \"MedicalDevice.name\", \"MedicalDevice.embeddedProductId\", \"AdministrableProduct.name\"]",
            ),
            (
                "CORE-000986",
                "[\"StudyIntervention.id\", \"StudyIntervention.name\", \"name\", \"administrableProductId\", \"AdministrableProduct.name\", \"medicalDeviceId\", \"MedicalDevice.name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Administration"] }} }},
  "Check": "study.versions.studyInterventions.administrations.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Administration rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          { "id": "AdmProd_1", "name": "Product 1" }
        ],
        "medicalDevices": [
          { "id": "MedDev_1", "name": "Device 1", "embeddedProductId": "AdmProd_1" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "name": "Intervention",
            "administrations": [
              {
                "id": "Administration_1",
                "name": "Route only",
                "instanceType": "Administration",
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_2",
                "name": "Dose without frequency or product",
                "instanceType": "Administration",
                "dose": {
                  "id": "Quantity_1",
                  "value": 30,
                  "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                },
                "route": {
                  "id": "Route_1",
                  "standardCode": { "decode": "Oral Route of Administration", "code": "C38288" }
                }
              },
              {
                "id": "Administration_3",
                "name": "Duplicated product",
                "instanceType": "Administration",
                "administrableProductId": "AdmProd_1",
                "medicalDeviceId": "MedDev_1"
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    for (id, expected_count) in [
        ("CORE-000966", 1),
        ("CORE-000967", 1),
        ("CORE-000969", 2),
        ("CORE-000986", 1),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run administration rule");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, expected_count);
        assert_eq!(outcome.results[0].errors[0].dataset, "Administration");
    }
}

#[test]
fn run_validation_executes_usdm_strength_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, output) in [
            (
                "CORE-001007",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.value\"]",
            ),
            (
                "CORE-001008",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"numerator.minValue\", \"numerator.maxValue\"]",
            ),
            (
                "CORE-001020",
                "[\"Ingredient.id\", \"Substance.id\", \"Substance.name\", \"name\", \"denominator.id\", \"denominator.value\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["Strength"] }} }},
  "Check": "study.versions.administrableProducts.ingredients.substance.strengths.{{\"check\": true}}",
  "Outcome": {{
    "Message": "Strength rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product 1",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Subst_1",
                  "name": "Substance 1",
                  "strengths": [
                    {
                      "id": "Strength_1",
                      "name": "Numerator value",
                      "instanceType": "Strength",
                      "numerator": { "id": "Quantity_1", "value": 10 }
                    },
                    {
                      "id": "Strength_2",
                      "name": "Numerator range",
                      "instanceType": "Strength",
                      "numerator": {
                        "minValue": { "id": "Quantity_2", "value": 50 },
                        "maxValue": {
                          "id": "Quantity_3",
                          "value": 100,
                          "unit": { "standardCode": { "decode": "Milligram", "code": "C28253" } }
                        }
                      }
                    },
                    {
                      "id": "Strength_3",
                      "name": "Denominator",
                      "instanceType": "Strength",
                      "denominator": { "id": "Quantity_4", "value": 2 }
                    }
                  ]
                }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    for id in ["CORE-001007", "CORE-001008", "CORE-001020"] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run strength rule");
        assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
        assert_eq!(outcome.results[0].error_count, 1);
        assert_eq!(outcome.results[0].errors[0].dataset, "Strength");
    }
}

#[test]
fn run_validation_executes_usdm_embedded_product_sourcing_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001001.json"),
            r#"{
  "Core": { "Id": "CORE-001001", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["AdministrableProduct"] } },
  "Check": "(study.versions)@$sv.$sv.administrableProducts@$ap.[{\"check\": true}]",
  "Outcome": {
    "Message": "The sourcing is defined while the administrable product is only referenced to as an embedded product for a medical device.",
    "Output Variables": [
      "name",
      "sourcing",
      "MedicalDevice.id",
      "MedicalDevice.name",
      "MedicalDevice.embeddedProductId"
    ]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "administrableProducts": [
          {
            "id": "AdministrableProduct_1",
            "name": "Embedded sourced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          },
          {
            "id": "AdministrableProduct_2",
            "name": "Embedded unsourced",
            "instanceType": "AdministrableProduct"
          },
          {
            "id": "AdministrableProduct_3",
            "name": "Admin referenced",
            "instanceType": "AdministrableProduct",
            "sourcing": { "code": "C123", "decode": "Manufactured" }
          }
        ],
        "medicalDevices": [
          { "id": "MedicalDevice_1", "name": "Device", "embeddedProductId": "AdministrableProduct_1" },
          { "id": "MedicalDevice_2", "name": "Other Device", "embeddedProductId": "AdministrableProduct_2" }
        ],
        "studyInterventions": [
          {
            "id": "StudyIntervention_1",
            "administrations": [
              { "id": "Administration_1", "administrableProductId": "AdministrableProduct_3" }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "AdministrableProduct");
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_usdm_reference_and_duplicate_jsonata_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
            (
                "CORE-000970",
                "StudyRole",
                "[\"name\", \"code\", \"appliesToIds\", \"StudyVersion.id\", \"StudyVersion.studyDesigns.id\"]",
            ),
            (
                "CORE-001022",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\", \"appliesTo name\"]",
            ),
            (
                "CORE-001024",
                "StudyDesign",
                "[\"name\", \"studyType\"]",
            ),
            (
                "CORE-001032",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001033",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001031",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\", \"primaryReason.code\"]",
            ),
            (
                "CORE-000999",
                "StudyDefinitionDocumentVersion",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"version\"]",
            ),
            (
                "CORE-000983",
                "Procedure",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"Activity.id\", \"Activity.name\", \"name\", \"studyInterventionId\", \"StudyIntervention.name\"]",
            ),
            (
                "CORE-000984",
                "SubjectEnrollment",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"name\", \"forGeographicScope\", \"forStudySiteId\", \"forStudyCohortId\"]",
            ),
            (
                "CORE-001010",
                "Substance",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Parent Substance.id\", \"Parent Substance.name\", \"name\", \"referenceSubstance.id\", \"referenceSubstance.name\"]",
            ),
            (
                "CORE-001018",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\"]",
            ),
            (
                "CORE-001019",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\", \"Used in\"]",
            ),
            (
                "CORE-001025",
                "BiospecimenRetention",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"isRetained\"]",
            ),
            (
                "CORE-001027",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"criterionItemId\"]",
            ),
            (
                "CORE-001028",
                "EligibilityCriterionItem",
                "[\"StudyVersion.id\", \"StudyVersion.versionIdentifier\", \"name\"]",
            ),
            (
                "CORE-001029",
                "StudyCohort",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.indications.id\", \"StudyDesignPopulation.id\", \"StudyDesignPopulation.name\", \"name\", \"Invalid indicationIds\"]",
            ),
            (
                "CORE-001030",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"name\", \"Invalid studyInterventionIds\", \"Invalid StudyIntervention.name\"]",
            ),
            (
                "CORE-001040",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"studyInterventionIds value\", \"Referenced intervention's parent StudyDesign.id\"]",
            ),
            (
                "CORE-001045",
                "StudyArm",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.population.id\", \"StudyDesign.population.cohorts.id\", \"name\", \"populationId\"]",
            ),
            (
                "CORE-001042",
                "GeographicScope",
                "[\"type.code\", \"type.decode\", \"code.standardCode.code\", \"code.standardCode.decode\"]",
            ),
            (
                "CORE-001051",
                "NarrativeContent",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"StudyDefinitionDocumentVersion.id\", \"StudyDefinitionDocumentVersion.version\", \"name\", \"sectionNumber\", \"sectionTitle\"]",
            ),
            (
                "CORE-001050",
                "NarrativeContent",
                "[\"StudyProtocolDocument.id\", \"StudyProtocolDocument.name\", \"StudyProtocolDocumentVersion.id\", \"StudyProtocolDocumentVersion.protocolVersion\", \"name\", \"sectionNumber\", \"sectionTitle\", \"Invalid Reference\"]",
            ),
            (
                "CORE-001023",
                "InterventionalStudyDesign",
                "[\"name\", \"intentTypes\"]",
            ),
            (
                "CORE-001046",
                "StudyDesign",
                "[\"id\", \"name\", \"interventionModel.code\", \"interventionModel.decode\", \"# Study Interventions\"]",
            ),
            (
                "CORE-001013",
                "USDMObject",
                "[\"name\"]",
            ),
            (
                "CORE-001015",
                "USDMObject",
                "[\"name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM reference rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Empty content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "1",
                "sectionTitle": "Overview"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Invalid ref content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "2",
                "sectionTitle": "Reference",
                "childIds": ["NarrativeContent_1"],
                "text": "<usdm:ref attribute=\"text\" id=\"MissingCriterion\" klass=\"EligibilityCriterion\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ],
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "1",
        "geographicScopes": [
          {
            "id": "GeographicScope_1",
            "name": "Global with code",
            "instanceType": "GeographicScope",
            "type": { "code": "C68846", "decode": "Global" },
            "code": { "standardCode": { "code": "US", "decode": "United States" } }
          }
        ],
        "duplicateObjects": [
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          },
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          }
        ],
        "studyInterventions": [
          { "id": "StudyIntervention_1", "name": "Valid intervention" },
          { "id": "StudyIntervention_2", "name": "Other intervention" }
        ],
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Substance_1",
                  "name": "Parent substance",
                  "referenceSubstance": {
                    "id": "Substance_2",
                    "name": "Reference substance",
                    "instanceType": "Substance",
                    "referenceSubstance": { "id": "Substance_3", "name": "Invalid nested reference" }
                  }
                }
              }
            ]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "ObservationalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "characteristics": [
              { "id": "Code_1", "code": "C217006", "decode": "Single Country" },
              { "id": "Code_2", "code": "C217007", "decode": "Multiple Countries" },
              { "id": "Code_3", "code": "C46079", "decode": "Randomized" },
              { "id": "Code_4", "code": "C25689", "decode": "Stratification" }
            ],
            "activities": [
              {
                "id": "Activity_1",
                "name": "Activity",
                "definedProcedures": [
                  {
                    "id": "Procedure_1",
                    "name": "Procedure",
                    "instanceType": "Procedure",
                    "studyInterventionId": "StudyIntervention_2"
                  }
                ]
              }
            ],
            "population": {
              "id": "Population_1",
              "name": "Population",
              "criterionIds": ["EligibilityCriterion_1"],
              "cohorts": [
                {
                  "id": "Cohort_1",
                  "name": "Cohort",
                  "criterionIds": ["EligibilityCriterion_1"],
                  "indicationIds": ["Indication_bad"]
                }
              ]
            },
            "indications": [{ "id": "Indication_1", "name": "Indication" }],
            "eligibilityCriteria": [
              {
                "id": "EligibilityCriterion_1",
                "name": "Criterion 1",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "01"
              },
              {
                "id": "EligibilityCriterion_2",
                "name": "Criterion 2",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "02"
              }
            ],
            "biospecimenRetentions": [
              {
                "id": "BiospecimenRetention_1",
                "name": "Retention",
                "instanceType": "BiospecimenRetention",
                "isRetained": true
              }
            ],
            "elements": [
              {
                "id": "StudyElement_1",
                "name": "Element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_2"]
              }
            ],
            "arms": [
              {
                "id": "StudyArm_1",
                "name": "Arm",
                "instanceType": "StudyArm",
                "populationIds": ["Population_bad"]
              }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Intent design",
            "instanceType": "InterventionalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "interventionModel": { "code": "C82640", "decode": "Single Group Design" },
            "studyInterventions": [
              { "id": "StudyDesignIntervention_1", "name": "Embedded intervention 1" },
              { "id": "StudyDesignIntervention_2", "name": "Embedded intervention 2" }
            ],
            "elements": [
              {
                "id": "StudyElement_2",
                "name": "Cross-design element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_1"]
              }
            ],
            "intentTypes": [
              { "id": "IntentType_1", "code": "C123", "decode": "Intent" },
              { "id": "IntentType_2", "code": "C123", "decode": "Intent duplicate" }
            ]
          }
        ],
        "eligibilityCriterionItems": [
          {
            "id": "EligibilityCriterionItem_unused",
            "name": "Unused criterion item",
            "instanceType": "EligibilityCriterionItem"
          }
        ],
        "roles": [
          {
            "id": "Role_1",
            "name": "Invalid role scope",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1", "StudyDesign_1"]
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "ProductRole_1",
            "name": "Invalid product role",
            "instanceType": "ProductOrganizationRole",
            "appliesToIds": ["StudyVersion_1"]
          }
        ],
        "amendments": [
          {
            "id": "Amendment_1",
            "name": "Amendment",
            "enrollments": [
              {
                "id": "Enrollment_1",
                "name": "Enrollment",
                "instanceType": "SubjectEnrollment"
              }
            ],
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C17649", "decode": "Other" }
            },
            "secondaryReasons": [
              {
                "id": "Reason_2",
                "instanceType": "StudyAmendmentReason",
                "code": { "code": "C17649", "decode": "Other" }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    for (id, dataset, expected_count) in [
        ("CORE-000970", "StudyRole", 1),
        ("CORE-001022", "ProductOrganizationRole", 1),
        ("CORE-001024", "StudyDesign", 1),
        ("CORE-001032", "StudyDesign", 1),
        ("CORE-001033", "StudyDesign", 1),
        ("CORE-001031", "StudyAmendmentReason", 1),
        ("CORE-000999", "StudyDefinitionDocumentVersion", 1),
        ("CORE-000983", "Procedure", 1),
        ("CORE-000984", "SubjectEnrollment", 1),
        ("CORE-001010", "Substance", 1),
        ("CORE-001018", "EligibilityCriterion", 1),
        ("CORE-001019", "EligibilityCriterion", 1),
        ("CORE-001025", "BiospecimenRetention", 1),
        ("CORE-001027", "EligibilityCriterion", 2),
        ("CORE-001028", "EligibilityCriterionItem", 1),
        ("CORE-001029", "StudyCohort", 1),
        ("CORE-001030", "StudyElement", 1),
        ("CORE-001040", "StudyElement", 2),
        ("CORE-001045", "StudyArm", 1),
        ("CORE-001042", "GeographicScope", 1),
        ("CORE-001051", "NarrativeContent", 1),
        ("CORE-001050", "NarrativeContent", 1),
        ("CORE-001023", "InterventionalStudyDesign", 1),
        ("CORE-001046", "StudyDesign", 1),
        ("CORE-001013", "USDMObject", 2),
        ("CORE-001015", "USDMObject", 2),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run reference rule");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{id}"
        );
        assert_eq!(outcome.results[0].error_count, expected_count, "{id}");
        assert_eq!(outcome.results[0].errors[0].dataset, dataset, "{id}");
    }
}

#[test]
fn run_validation_executes_usdm_id_contains_space_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-001075.json"),
            r#"{
  "Core": { "Id": "CORE-001075", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ALL"] } },
  "Check": "(**[$contains($string(id),\" \")])@$i.{\"instanceType\": $i.instanceType,\"id\": $join(['\"','\"'],$i.id),\"path\": $i._path,\"name\": $i.name}",
  "Outcome": {
    "Message": "The id value contains a space.",
    "Output Variables": ["name"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "id": "Study 1",
    "name": "Study with spaced id",
    "instanceType": "Study",
    "versions": [
      {
        "id": "StudyVersion_1",
        "name": "Clean version",
        "instanceType": "StudyVersion"
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "USDMObject");
    assert_eq!(outcome.results[0].errors[0].variables, vec!["name"]);
}

#[test]
fn run_validation_executes_usdm_study_identifier_duplicate_scope_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000956.json"),
            r#"{
  "Core": { "Id": "CORE-000956", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["StudyIdentifier"] } },
  "Check": "study.versions@$sv.($sv.organizations{id:($o:=$;$i:=$sv.studyIdentifiers[scopeId=$o.id];$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "More than 1 study identifier is specified for the same organization.",
    "Output Variables": ["text", "scopeId", "Organization.name"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "studyIdentifiers": [
          { "id": "StudyIdentifier_1", "instanceType": "StudyIdentifier", "text": "ABC-001", "scopeId": "Organization_1" },
          { "id": "StudyIdentifier_2", "instanceType": "StudyIdentifier", "text": "NCT-001", "scopeId": "Organization_1" }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "StudyIdentifier");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["text", "scopeId", "Organization.name"]
    );
}

#[test]
fn run_validation_executes_usdm_identifier_text_duplicate_scope_jsonata_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000955.json"),
            r#"{
  "Core": { "Id": "CORE-000955", "Status": "Published" },
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": { "Entities": { "Include": ["ReferenceIdentifier"] } },
  "Check": "study.versions@$sv.($sv.**.*[scopeId and text and instanceType]{$join([text,scopeId,instanceType],\"\\n\"):($i:=$;$count($i)>1 ? $i.{\"check\": true})}).*",
  "Outcome": {
    "Message": "The identifier text is not unique within the scope of the identified organization.",
    "Output Variables": ["text", "scopeId", "Organization.name", "type.decode"]
  }
}"#,
        )
        .expect("write rule");
    fs::write(
        data_dir.join("usdm.json"),
        r#"{
  "study": {
    "versions": [
      {
        "organizations": [
          { "id": "Organization_1", "name": "Sponsor", "instanceType": "Organization" }
        ],
        "referenceIdentifiers": [
          {
            "id": "ReferenceIdentifier_1",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Clinical Development Plan" }
          },
          {
            "id": "ReferenceIdentifier_2",
            "instanceType": "ReferenceIdentifier",
            "text": "PLAN-001",
            "scopeId": "Organization_1",
            "type": { "decode": "Pediatric Investigation Plan" }
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("write json");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].dataset, "ReferenceIdentifier");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["text", "scopeId", "Organization.name", "type.decode"]
    );
}

#[test]
fn run_validation_uses_define_xml_and_ct_for_codelist_checks() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-CT-DOMAIN.json"),
        r#"{
  "Core": { "Id": "CORE-CT-DOMAIN", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "is_not_contained_by"
  },
  "Outcome": { "Message": "DOMAIN must use controlled terminology" }
}"#,
    )
    .expect("write codelist rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "DOMAIN": ["AE", "CM", "XX"],
        "AESEQ": [1, 2, 3]
      }
    }
  ]
}"#,
    )
    .expect("write codelist data");

    let define_xml_path = dir.path().join("define.xml");
    fs::write(
        &define_xml_path,
        r#"
<ODM>
  <ItemDef OID="IT.DOMAIN" Name="DOMAIN" DataType="text">
    <CodeListRef CodeListOID="CL.DOMAIN"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN">
    <CodeListItem CodedValue="AE"/>
  </CodeList>
</ODM>
"#,
    )
    .expect("write define xml");
    let ct_path = dir.path().join("ct.json");
    fs::write(&ct_path, r#"{ "CL.DOMAIN": ["CM"] }"#).expect("write ct");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        define_xml_paths: vec![define_xml_path],
        ct_paths: vec![ct_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(3));
}

#[test]
fn run_validation_resolves_define_and_ct_codelist_aliases() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-CT-ALIAS.json"),
        r#"{
  "Core": { "Id": "CORE-CT-ALIAS", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AE.DOMAIN",
    "operator": "is_not_contained_by"
  },
  "Outcome": { "Message": "DOMAIN must use Define-XML and CT terminology" }
}"#,
    )
    .expect("write codelist rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S2", "S3"],
        "AE.DOMAIN": ["AE", "CM", "XX"],
        "AESEQ": [1, 2, 3]
      }
    }
  ]
}"#,
    )
    .expect("write codelist data");

    let define_xml_path = dir.path().join("define.xml");
    fs::write(
        &define_xml_path,
        r#"
<odm:ODM xmlns:odm="http://www.cdisc.org/ns/odm/v1.3">
  <odm:ItemDef OID="IT.DOMAIN" Name="DOMAIN" DataType="text">
    <odm:CodeListRef CodeListOID="CL.DOMAIN"/>
  </odm:ItemDef>
  <odm:CodeList OID="CL.DOMAIN" Name="Domain Abbreviation" SASFormatName="DOMAIN">
    <odm:CodeListItem CodedValue="AE"/>
  </odm:CodeList>
</odm:ODM>
"#,
    )
    .expect("write define xml");
    let ct_path = dir.path().join("ct.json");
    fs::write(
        &ct_path,
        r#"{
  "codelists": [
    {
      "submissionValue": "DOMAIN",
      "conceptId": "C66734",
      "terms": [
        { "submissionValue": "CM" }
      ]
    }
  ]
}"#,
    )
    .expect("write ct");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        define_xml_paths: vec![define_xml_path],
        ct_paths: vec![ct_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(3));
}

#[test]
fn define_codelist_resolution_uses_domain_and_avoids_ambiguous_globals() {
    let dir = tempdir().expect("tempdir");
    let define_xml_path = dir.path().join("define.xml");
    fs::write(
        &define_xml_path,
        r#"
<ODM>
  <ItemGroupDef OID="IG.AE" Name="AE" Domain="AE">
    <ItemRef ItemOID="IT.AE.PARAMCD"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.CM" Name="CM" Domain="CM">
    <ItemRef ItemOID="IT.CM.PARAMCD"/>
  </ItemGroupDef>
  <ItemDef OID="IT.AE.PARAMCD" Name="PARAMCD" DataType="text">
    <CodeListRef CodeListOID="CL.AE.PARAMCD"/>
  </ItemDef>
  <ItemDef OID="IT.CM.PARAMCD" Name="PARAMCD" DataType="text">
    <CodeListRef CodeListOID="CL.CM.PARAMCD"/>
  </ItemDef>
</ODM>
"#,
    )
    .expect("write define xml");
    let context = CdiscContext::load(&[define_xml_path], &[], &[]).expect("load context");

    let unqualified = Condition {
        target: Some("PARAMCD".to_owned()),
        operator: Operator::IsContainedBy,
        comparator: ValueExpr::Null,
        options: Default::default(),
    };
    assert_eq!(define_codelist_for_condition(&unqualified, &context), None);

    let qualified = Condition {
        target: Some("AE.PARAMCD".to_owned()),
        operator: Operator::IsContainedBy,
        comparator: ValueExpr::Null,
        options: Default::default(),
    };
    assert_eq!(
        define_codelist_for_condition(&qualified, &context),
        Some("CL.AE.PARAMCD".to_owned())
    );
}

#[test]
fn run_validation_uses_external_dictionary_for_term_checks() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DICTIONARY-MEDDRA.json"),
        r#"{
  "Core": { "Id": "CORE-DICTIONARY-MEDDRA", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "AEDECOD",
    "operator": "is_not_contained_by",
    "dictionary": "MEDDRA"
  },
  "Outcome": { "Message": "AEDECOD must exist in external dictionary" }
}"#,
    )
    .expect("write dictionary rule");

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
        "AESEQ": [1, 2],
        "AEDECOD": ["HEADACHE", "UNKNOWN"]
      }
    }
  ]
}"#,
    )
    .expect("write dictionary data");

    let dictionary_path = dir.path().join("external_dictionary.json");
    fs::write(
        &dictionary_path,
        r#"{
  "dictionaries": [
    {
      "dictionary": "MEDDRA",
      "terms": [
        { "term": "HEADACHE" },
        { "term": "NAUSEA" }
      ]
    }
  ]
}"#,
    )
    .expect("write dictionary");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        external_dictionary_paths: vec![dictionary_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
}

#[test]
fn run_validation_filters_rules_by_standard_and_version() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-STANDARD-34.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write matching standard rule");
    fs::write(
        rules_dir.join("CORE-STANDARD-33.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "CM"
  },
  "Outcome": { "Message": "DOMAIN must be CM" }
}"#,
    )
    .expect("write nonmatching standard rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-34");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_reports_explicit_rule_standard_mismatch_as_skipped() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-STANDARD-33.json"),
        r#"{
  "Core": { "Id": "CORE-STANDARD-33", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.3" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write nonmatching standard rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-STANDARD-33".to_owned()],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-STANDARD-33");
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::StandardMismatch)
    );
}

#[test]
fn run_validation_classifies_known_standard_filter_fixture_gaps_as_oracle_gaps() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-000478.json"),
        r#"{
  "Core": { "Id": "CORE-000478", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "name": "DOMAIN", "operator": "not_equal_to", "value": "AE" },
  "Outcome": { "Message": "known standard filter fixture gap" }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000478".to_owned()],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.0".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OracleSemanticsGap)
    );
}

#[test]
fn run_validation_treats_usdm_30_rules_as_compatible_with_usdm_40_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-USDM-30.json"),
        r#"{
  "Core": { "Id": "CORE-USDM-30", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "USDM", "Version": "3.0" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write USDM 3.0 rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("USDM".to_owned()),
        standard_version: Some("4.0".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-USDM-30");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_sdtmig_34_rules_as_compatible_with_sdtmig_33_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-SDTMIG-34.json"),
        r#"{
  "Core": { "Id": "CORE-SDTMIG-34", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write SDTMIG 3.4 rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.3".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-SDTMIG-34");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_send_family_versions_as_compatible_for_open_rules_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-SEND-FAMILY.json"),
        r#"{
  "Core": { "Id": "CORE-SEND-FAMILY", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG-DART", "Version": "1.1" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write SEND family rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-SEND-FAMILY");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_core_000119_tig_sdtm_as_compatible_with_sendig_31() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "ARMCD", "label": "Planned Arm Code", "type": "Char", "length": 20 },
        { "name": "ARM", "label": "Description of Planned Arm", "type": "Char", "length": 40 }
      ],
      "records": { "ARMCD": [""], "ARM": ["PLACEBO"] }
    }
  ]
}"#,
    )
    .expect("write data");

    fs::write(
        rules_dir.join("CORE-000119.json"),
        r#"{
  "Core": { "Id": "CORE-000119", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "all": [
    { "name": "ARMCD", "operator": "empty" },
    { "name": "ARM", "operator": "non_empty" }
  ] },
  "Outcome": {
    "Message": "ARM is populated, when ARMCD is NULL",
    "Output Variables": ["ARMCD", "ARM"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-000119");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_treats_tig_rules_as_compatible_with_sdtmig_fixtures() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    let dataset_path = write_dataset(&data_dir);

    fs::write(
        rules_dir.join("CORE-TIG.json"),
        r#"{
  "Core": { "Id": "CORE-TIG", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "TIG", "Version": "1.0", "Substandard": "SDTM" }] }
  ],
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "DOMAIN",
    "operator": "not_equal_to",
    "value": "AE"
  },
  "Outcome": { "Message": "DOMAIN must be AE" }
}"#,
    )
    .expect("write TIG rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].rule_id, "CORE-TIG");
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
}

#[test]
fn run_validation_executes_filter_sort_aggregate_and_derive_operations() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-PIPELINE.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-PIPELINE", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "filter",
      "dataset": "AE",
      "where": {
        "name": "AESER",
        "operator": "equal_to",
        "value": "Y"
      }
    },
    {
      "name": "sort",
      "by": ["AESEQ"],
      "descending": true
    },
    {
      "name": "aggregate",
      "by": ["USUBJID"],
      "as": "USUBJID_COUNT"
    },
    {
      "name": "derive",
      "as": "PIPELINE",
      "value": "OPS"
    }
  ],
  "Check": {
    "all": [
      {
        "name": "USUBJID_COUNT",
        "operator": "greater_than",
        "value": 1
      },
      {
        "name": "PIPELINE",
        "operator": "equal_to",
        "value": "OPS"
      }
    ]
  },
  "Outcome": { "Message": "Duplicate serious AE subject requires review" }
}"#,
    )
    .expect("write operations rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S2", "S1", "S2"],
        "DOMAIN": ["AE", "AE", "AE"],
        "AESEQ": [2, 1, 3],
        "AESER": ["Y", "N", "Y"]
      }
    }
  ]
}"#,
    )
    .expect("write operations data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("3"));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
    assert_eq!(outcome.results[0].errors[1].seq.as_deref(), Some("2"));
}

#[test]
fn run_validation_executes_expanded_operations_pipeline() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-EXPANDED.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-EXPANDED", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "name": "derive",
      "dataset": "AE",
      "as": "TERM_TRIM",
      "expression": "$trim(AETERM)"
    },
    {
      "name": "derive",
      "as": "TERM_UP",
      "expression": "$uppercase(TERM_TRIM)"
    },
    {
      "name": "aggregate",
      "by": ["USUBJID"],
      "function": "sum",
      "source_column": "AVAL",
      "as": "AVAL_SUM"
    },
    {
      "name": "distinct",
      "by": ["USUBJID", "TERM_UP", "AVAL_SUM"]
    },
    {
      "name": "rename",
      "columns": { "TERM_UP": "TERM" }
    },
    {
      "name": "row_number",
      "by": ["USUBJID"],
      "as": "ROWNUM"
    },
    {
      "name": "select",
      "columns": ["USUBJID", "AESEQ", "TERM", "AVAL_SUM", "ROWNUM"]
    }
  ],
  "Check": {
    "all": [
      {
        "name": "AVAL_SUM",
        "operator": "greater_than",
        "value": 4
      },
      {
        "name": "TERM",
        "operator": "equal_to",
        "value": "HEADACHE"
      },
      {
        "name": "ROWNUM",
        "operator": "equal_to",
        "value": 1
      }
    ]
  },
  "Outcome": { "Message": "High aggregate value requires review" }
}"#,
    )
    .expect("write expanded operations rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1", "S2"],
        "AESEQ": [1, 2, 3],
        "AETERM": [" headache ", "headache", "nausea"],
        "AVAL": [2, 3, 1]
      }
    }
  ]
}"#,
    )
    .expect("write expanded operations data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[0].seq.as_deref(), Some("1"));
}

#[test]
fn run_validation_executes_open_rules_operator_style_record_count_and_distinct() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-OPEN-RULES.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-OPEN-RULES", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "GS",
      "group": ["PARENT", "REL"],
      "id": "$COUNT"
    },
    {
      "operator": "distinct",
      "group": ["PARENT", "REL"],
      "id": "$SCOPES",
      "name": "SCOPE"
    }
  ],
  "Check": {
    "all": [
      { "name": "$COUNT", "operator": "greater_than", "value": 1 },
      { "name": "$SCOPES", "operator": "contains_case_insensitive", "value": "global" }
    ]
  },
  "Outcome": { "Message": "Global scope appears more than once" }
}"#,
    )
    .expect("write operations rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "gs.xpt",
      "domain": "GS",
      "records": {
        "PARENT": ["A", "A", "B"],
        "REL": ["definition", "definition", "definition"],
        "SCOPE": ["Global", "Regional", "Regional"]
      }
    }
  ]
}"#,
    )
    .expect("write operations data");

    let rules = load_rules_from_paths(std::slice::from_ref(&rules_dir)).expect("load rules");
    assert_eq!(rules[0].operations.len(), 2);
    assert_eq!(
        operation_name(&rules[0].operations[0]).as_deref(),
        Some("record_count")
    );
    assert_eq!(
        operation_name(&rules[0].operations[1]).as_deref(),
        Some("distinct")
    );

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_executes_grouped_distinct_operation_for_required_txparmcd() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000891.json"),
        r#"{
  "Core": { "Id": "CORE-000891", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "ARMCD"
  },
  "Outcome": {
    "Message": "TX dataset should include a TXPARMCD = ARMCD record per SETCD.",
    "Output Variables": ["SETCD", "$txparmcd"]
  }
}"#,
    )
    .expect("write grouped distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "B", "B"],
        "TXPARMCD": ["ARMCD", "SPECIES", "ARMCDxxx", "STRAIN"]
      }
    }
  ]
}"#,
    )
    .expect("write grouped distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(3));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["SETCD".to_owned(), "$txparmcd".to_owned()]
    );
}

#[test]
fn run_validation_executes_group_sensitivity_for_grouped_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000888.json"),
        r#"{
  "Core": { "Id": "CORE-000888", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Group",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }
  ],
  "Check": {
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "PLANFSUB"
  },
  "Outcome": {
    "Message": "TX dataset should include exactly one TXPARMCD = 'PLANFSUB' record per SETCD.",
    "Output Variables": ["SETCD", "$txparmcd"]
  }
}"#,
    )
    .expect("write group sensitivity rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["SET1", "SET1", "SET2", "SET2"],
        "TXPARMCD": ["ARMCD", "PLANFSUB", "ARMCD", "SPGRPCD"]
      }
    }
  ]
}"#,
    )
    .expect("write group sensitivity data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Failed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "TX");
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["SETCD".to_owned(), "$txparmcd".to_owned()]
    );
}

#[test]
fn run_validation_executes_grouped_distinct_operation_for_treatment_dose_parms() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (rule_id, required_txparmcd) in [("CORE-000894", "TRTDOS"), ("CORE-000895", "TRTDOSU")] {
        fs::write(
            rules_dir.join(format!("{rule_id}.json")),
            format!(
                r#"{{
  "Core": {{ "Id": "{rule_id}", "Status": "Published" }},
  "Scope": {{ "Domains": {{ "Include": ["TX"] }} }},
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {{
      "domain": "TX",
      "group": ["SETCD"],
      "id": "$txparmcd",
      "name": "TXPARMCD",
      "operator": "distinct"
    }}
  ],
  "Check": {{
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "{required_txparmcd}"
  }},
  "Outcome": {{
    "Message": "TX dataset should include a TXPARMCD = {required_txparmcd} record per SETCD."
  }}
}}"#
            ),
        )
        .expect("write grouped distinct treatment dose rule");
    }

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "B", "B", "B"],
        "TXPARMCD": ["TRTDOS", "TRTDOSU", "ARMCD", "TRTDOSxxx", "TRTDOSUxxx"]
      }
    }
  ]
}"#,
    )
    .expect("write grouped distinct treatment dose data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    for result in outcome.results {
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 3);
        assert_eq!(result.errors[0].row, Some(3));
        assert_eq!(result.errors[0].variables, vec!["$txparmcd".to_owned()]);
    }
}

#[test]
fn run_validation_executes_open_rules_distinct_with_schema_normalized_keys() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DISTINCT-SCHEMA-KEYS.yml"),
        r#"Core:
  Id: CORE-DISTINCT-SCHEMA-KEYS
  Status: Published
Scope:
  Domains:
    Include:
      - ACTIVITY
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - parent_id
    id: $activity_ids_for_parent
    name: id
    operator: distinct
Check:
  name: $activity_ids_for_parent
  operator: contains
  value: Activity_2
Outcome:
  Message: Parent contains Activity_2
"#,
    )
    .expect("write distinct schema rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Dataset,Label\nActivity.csv,Activity,Activity\n",
    )
    .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nActivity,parent_id,Parent Entity Id,String,[1]\nActivity,id,Activity Id,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
        data_dir.join("Activity.csv"),
        "parent_id,id\nDesign_1,Activity_1\nDesign_1,Activity_2\nDesign_2,Activity_3\n",
    )
    .expect("write activity csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_core_000837_entity_column_ref_distinct_set() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000837.yml"),
        r#"Core:
  Id: CORE-000837
  Status: Published
Scope:
  Entities:
    Include:
      - Activity
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - parent_id
      - rel_type
    id: $activity_ids_for_study_design
    name: id
    operator: distinct
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: Activity
    - name: rel_type
      operator: equal_to
      value: definition
    - any:
        - all:
            - name: previousId
              operator: exists
            - name: previousId
              operator: non_empty
            - name: previousId
              operator: is_not_contained_by
              value: $activity_ids_for_study_design
        - all:
            - name: nextId
              operator: exists
            - name: nextId
              operator: non_empty
            - name: nextId
              operator: is_not_contained_by
              value: $activity_ids_for_study_design
Outcome:
  Message: Activity references must stay within the same study design
  Output Variables:
    - id
    - parent_id
    - previousId
    - nextId
    - $activity_ids_for_study_design
"#,
    )
    .expect("write entity column-ref rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Dataset,Label\nActivity.csv,Activity,Activity\n",
    )
    .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nActivity,parent_id,Parent Entity Id,String,[1]\nActivity,rel_type,Type of Relationship,String,[1]\nActivity,id,Activity Id,String,[1]\nActivity,instanceType,Instance Type,String,[1]\nActivity,previousId,Previous Activity,String,[0..1]\nActivity,nextId,Next Activity,String,[0..1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("Activity.csv"),
            "parent_id,rel_type,id,instanceType,previousId,nextId\nDesign_1,definition,Activity_1,Activity,,Activity_2\nDesign_1,definition,Activity_2,Activity,Activity_1,Activity_3\nDesign_2,definition,Activity_3,Activity,Activity_2,\n",
        )
        .expect("write activity csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[1].row, Some(3));
}

#[test]
fn run_validation_executes_core_000427_record_count_column_ref_comparisons() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000427.yml"),
        r#"Core:
  Id: CORE-000427
  Status: Published
Scope:
  Entities:
    Include:
      - Code
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - codeSystem
      - codeSystemVersion
      - code
    id: $num_records_in_codesystemversion_with_code
    operator: record_count
  - group:
      - codeSystem
      - codeSystemVersion
      - decode
    id: $num_records_in_codesystemversion_with_decode
    operator: record_count
  - group:
      - codeSystem
      - codeSystemVersion
      - code
      - decode
    id: $num_records_in_codesystemversion_with_code_decode
    operator: record_count
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: Code
    - any:
        - name: $num_records_in_codesystemversion_with_code
          operator: not_equal_to
          value: $num_records_in_codesystemversion_with_code_decode
        - name: $num_records_in_codesystemversion_with_decode
          operator: not_equal_to
          value: $num_records_in_codesystemversion_with_code_decode
Outcome:
  Message: Code and decode should have a one-to-one relationship
  Output Variables:
    - codeSystem
    - codeSystemVersion
    - code
    - decode
    - $num_records_in_codesystemversion_with_code
    - $num_records_in_codesystemversion_with_decode
    - $num_records_in_codesystemversion_with_code_decode
"#,
    )
    .expect("write record count column-ref rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Dataset,Label\nCode.csv,Code,Code\n",
    )
    .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nCode,codeSystem,Code System,String,[1]\nCode,codeSystemVersion,Code System Version,String,[1]\nCode,code,Code,String,[1]\nCode,decode,Decode,String,[1]\nCode,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("Code.csv"),
            "codeSystem,codeSystemVersion,code,decode,instanceType\nCDISC,2024-01,A,Alpha,Code\nCDISC,2024-01,A,Beta,Code\nCDISC,2024-01,B,Gamma,Code\n",
        )
        .expect("write code csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_record_count_operation_inline_filter() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-FILTERED-RECORD-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-FILTERED-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TX",
      "filter": { "TXPARMCD": "ARMCD" },
      "group": ["SETCD"],
      "id": "$armcd_count"
    }
  ],
  "Check": {
    "name": "$armcd_count",
    "operator": "greater_than",
    "value": 1
  },
  "Outcome": {
    "Message": "There may be only one ARMCD per SETCD",
    "Output Variables": ["SETCD", "$armcd_count", "TXPARMCD"]
  }
}"#,
    )
    .expect("write record count rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "SETCD": ["A", "A", "A", "B", "B"],
        "TXPARMCD": ["ARMCD", "ARMCD", "SPECIES", "ARMCD", "SPECIES"]
      }
    }
  ]
}"#,
    )
    .expect("write record count data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 3);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
    assert_eq!(outcome.results[0].errors[2].row, Some(3));
}

#[test]
fn run_validation_preserves_multi_domain_scope_after_targeted_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-MULTI-DOMAIN-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-MULTI-DOMAIN-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "TS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGEU" },
      "id": "$ageu_count"
    }
  ],
  "Check": {
    "any": [
      {
        "all": [
          { "name": "DOMAIN", "operator": "equal_to", "value": "DM" },
          { "name": "AGEU", "operator": "empty" },
          { "name": "AGE", "operator": "non_empty" }
        ]
      },
      {
        "all": [
          { "name": "DOMAIN", "operator": "equal_to", "value": "TS" },
          { "name": "$ageu_count", "operator": "equal_to", "value": 0 }
        ]
      }
    ]
  },
  "Outcome": {
    "Message": "AGEU is expected when AGE is populated",
    "Output Variables": ["DOMAIN", "AGE", "AGEU", "$ageu_count"]
  }
}"#,
    )
    .expect("write multi-domain rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["DM1", "DM2"],
        "DOMAIN": ["DM", "DM"],
        "AGE": ["33", ""],
        "AGEU": ["", "YRS"]
      }
    },
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["TS1", "TS2"],
        "DOMAIN": ["TS", "TS"],
        "TSPARMCD": ["AGEU", "AGE"],
        "TSVAL": ["YRS", "33"]
      }
    }
  ]
}"#,
    )
    .expect("write multi-domain data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    let dm_result = outcome
        .results
        .iter()
        .find(|result| result.domain.as_deref() == Some("DM"))
        .expect("DM result");
    let ts_result = outcome
        .results
        .iter()
        .find(|result| result.domain.as_deref() == Some("TS"))
        .expect("TS result");

    assert_eq!(dm_result.execution_status, ExecutionStatus::Failed);
    assert_eq!(dm_result.error_count, 1);
    assert_eq!(dm_result.errors[0].row, Some(1));
    assert_eq!(ts_result.execution_status, ExecutionStatus::Passed);
    assert_eq!(ts_result.error_count, 0);
}

#[test]
fn run_validation_executes_open_rules_record_count_with_schema_normalized_keys() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-RECORD-COUNT-SCHEMA-KEYS.yml"),
        r#"Core:
  Id: CORE-RECORD-COUNT-SCHEMA-KEYS
  Status: Published
Scope:
  Domains:
    Include:
      - CODE
Sensitivity: Record
Rule Type: Record Data
Operations:
  - group:
      - codeSystem
      - codeSystemVersion
      - code
    id: $num_records_with_code
    operator: record_count
Check:
  name: $num_records_with_code
  operator: greater_than
  value: 1
Outcome:
  Message: Duplicate code within a code system version
"#,
    )
    .expect("write record count schema rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Dataset,Label\nCode.csv,Code,Code\n",
    )
    .expect("write datasets csv");
    fs::write(
            data_dir.join("_variables.csv"),
            "dataset,variable,label,type,length\nCode,codeSystem,Code System,String,[1]\nCode,codeSystemVersion,Code System Version,String,[1]\nCode,code,Code,String,[1]\nCode,instanceType,Instance Type,String,[1]\n",
        )
        .expect("write variables csv");
    fs::write(
            data_dir.join("Code.csv"),
            "codeSystem,codeSystemVersion,code,instanceType\nCDISC,2024-01,X,Code\nCDISC,2024-01,X,Code\nCDISC,2024-01,Y,Code\n",
        )
        .expect("write code csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(outcome.results[0].errors[1].row, Some(2));
}

#[test]
fn run_validation_executes_record_count_operation_without_group() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-OPS-DATASET-RECORD-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-OPS-DATASET-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGE" },
      "id": "$record_count_AGE"
    },
    {
      "operator": "record_count",
      "domain": "TS",
      "filter": { "TSPARMCD": "AGETXT" },
      "id": "$record_count_AGETXT"
    }
  ],
  "Check": {
    "all": [
      { "name": "$record_count_AGE", "operator": "greater_than_or_equal_to", "value": 1 },
      { "name": "$record_count_AGETXT", "operator": "greater_than_or_equal_to", "value": 1 }
    ]
  },
  "Outcome": {
    "Message": "AGE and AGETXT must not both be present",
    "Output Variables": ["$record_count_AGE", "$record_count_AGETXT"]
  }
}"#,
    )
    .expect("write dataset record count rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "DOMAIN": ["TS", "TS", "TS"],
        "TSPARMCD": ["AGE", "AGETXT", "SEX"]
      }
    }
  ]
}"#,
    )
    .expect("write dataset record count data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, None);
}

#[test]
fn run_validation_maps_external_record_count_operation_by_group_aliases() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-ENTITY-RECORD-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-ENTITY-RECORD-COUNT", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyVersion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "domain": "StudyIdentifier",
      "filter": {
        "parent_entity": "StudyVersion",
        "parent_rel": "studyIdentifiers",
        "rel_type": "definition",
        "studyIdentifierScope.organizationType.code": "C70793",
        "studyIdentifierScope.organizationType.codeSystem": "http://www.cdisc.org"
      },
      "group": ["parent_id"],
      "group_aliases": ["id"],
      "id": "$num_sponsor_ids",
      "operator": "record_count"
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyVersion" },
      {
        "any": [
          { "name": "$num_sponsor_ids", "operator": "empty" },
          { "name": "$num_sponsor_ids", "operator": "not_equal_to", "value": 1 }
        ]
      }
    ]
  },
  "Outcome": { "Message": "StudyVersion must have exactly one sponsor identifier" }
}"#,
    )
    .expect("write external record count rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
            &dataset_path,
            r#"{
  "datasets": [
    {
      "filename": "StudyVersion.csv",
      "domain": "StudyVersion",
      "records": {
        "ID": ["StudyVersion_1", "StudyVersion_2", "StudyVersion_3"],
        "instanceType": ["StudyVersion", "StudyVersion", "StudyVersion"]
      }
    },
    {
      "filename": "StudyIdentifier.csv",
      "domain": "StudyIdentifier",
      "records": {
        "parent_entity": ["StudyVersion", "StudyVersion", "StudyVersion", "StudyVersion"],
        "PARENT_ID": ["StudyVersion_1", "StudyVersion_1", "StudyVersion_2", "StudyVersion_3"],
        "parent_rel": ["studyIdentifiers", "studyIdentifiers", "studyIdentifiers", "studyIdentifiers"],
        "rel_type": ["definition", "definition", "definition", "definition"],
        "studyIdentifierScope.organizationType.code": ["C70793", "C70793", "C70793", "C93453"],
        "studyIdentifierScope.organizationType.codeSystem": ["http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org", "http://www.cdisc.org"]
      }
    }
  ]
}"#,
        )
        .expect("write external record count data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_skips_operation_oracle_gap_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000894.json"),
        r#"{
  "Core": { "Id": "CORE-000894", "Status": "Published" },
  "Scope": { "Domains": {}, "Classes": {} },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "operator": "distinct",
      "domain": "GS",
      "group": ["PARENT"],
      "name": "REL",
      "id": "$VALUES"
    }
  ],
  "Check": { "name": "$VALUES", "operator": "does_not_contain", "value": "global" },
  "Outcome": { "Message": "distinct semantics are not oracle-compatible yet" }
}"#,
    )
    .expect("write operation gap rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "gs.xpt",
      "domain": "GS",
      "records": {
        "PARENT": ["A", "A"]
      }
    }
  ]
}"#,
    )
    .expect("write operations data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Skipped
    );
    assert_eq!(
        outcome.results[0].skipped_reason,
        Some(SkippedReason::OperationsNotSupported)
    );
}

fn write_test_xpt_char_dataset(
    path: &std::path::Path,
    dataset_name: &str,
    columns: &[&str],
    rows: &[Vec<&str>],
) {
    const CARD_LEN: usize = 80;
    const NAMESTR_LEN: usize = 140;

    let mut bytes = Vec::new();
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        "SAS     SAS     SASLIB  9.4     X64_10PRO                       18JUN26:00:00:00",
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******MEMBER  HEADER RECORD!!!!!!!000000000000000001600000000140",
    );
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******DSCRPTR HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        &format!(
            "SAS     {:<8}SASDATA 9.4     X64_10PRO                       18JUN26:00:00:00",
            dataset_name
        ),
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        &format!(
            "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
            columns.len()
        ),
    );

    let lengths = columns
        .iter()
        .map(|column| match *column {
            "DOMAIN" => 2,
            "AESEQ" | "CMSEQ" | "SEQ" => 8,
            _ => 12,
        })
        .collect::<Vec<_>>();
    let mut offset = 0_u32;
    let mut namestrs = Vec::new();
    for (index, (column, length)) in columns.iter().zip(&lengths).enumerate() {
        let mut namestr = vec![0_u8; NAMESTR_LEN];
        namestr[0..2].copy_from_slice(&2_u16.to_be_bytes());
        namestr[4..6].copy_from_slice(&(*length as u16).to_be_bytes());
        namestr[6..8].copy_from_slice(&((index + 1) as u16).to_be_bytes());
        write_padded(&mut namestr[8..16], column);
        write_padded(&mut namestr[16..56], column);
        namestr[84..88].copy_from_slice(&offset.to_be_bytes());
        offset += *length as u32;
        namestrs.extend(namestr);
    }
    pad_to_xpt_card(&mut namestrs);
    bytes.extend(namestrs);

    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******OBS     HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    for row in rows {
        assert_eq!(row.len(), columns.len());
        for (value, length) in row.iter().zip(&lengths) {
            let start = bytes.len();
            bytes.resize(start + *length, b' ');
            write_padded(&mut bytes[start..start + *length], value);
        }
    }
    pad_to_xpt_card(&mut bytes);

    fs::write(path, bytes).expect("write xpt");

    fn push_xpt_card(bytes: &mut Vec<u8>, value: &str) {
        let start = bytes.len();
        bytes.resize(start + CARD_LEN, b' ');
        write_padded(&mut bytes[start..start + CARD_LEN], value);
    }

    fn write_padded(target: &mut [u8], value: &str) {
        let bytes = value.as_bytes();
        let len = bytes.len().min(target.len());
        target[..len].copy_from_slice(&bytes[..len]);
    }

    fn pad_to_xpt_card(bytes: &mut Vec<u8>) {
        let remainder = bytes.len() % CARD_LEN;
        if remainder != 0 {
            bytes.resize(bytes.len() + CARD_LEN - remainder, b' ');
        }
    }
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
