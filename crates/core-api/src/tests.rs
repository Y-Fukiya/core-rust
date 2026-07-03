use std::{collections::BTreeSet, fs};

use core_engine::ExecutionStatus;
use core_rule_model::{load_rules_from_paths, Sensitivity};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;
use helpers::{write_dataset, write_raw_rule, write_rule};

mod basic_validation;
mod helpers;
mod open_rules_codelists;
mod open_rules_data_loader;
mod open_rules_dates;
mod open_rules_entities;
mod open_rules_jsonata;
mod open_rules_match_datasets;
mod open_rules_metadata;
mod open_rules_operations;
mod open_rules_operations_record_count;
mod open_rules_oracle_semantics;
mod open_rules_reference_distinct;
mod open_rules_scope;
mod open_rules_standard_filter;
mod open_rules_usdm;
mod open_rules_usdm_abbreviations;
mod open_rules_usdm_activity;
mod open_rules_usdm_blinding;
mod open_rules_usdm_identifiers;
mod open_rules_usdm_narrative;
mod open_rules_usdm_population;
mod open_rules_usdm_references;
mod open_rules_usdm_schema;
mod open_rules_usdm_study_design;
mod open_rules_usdm_timeline;

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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 2);
}

#[test]
fn run_validation_core_000750_detects_unique_set_duplicates_across_split_domains() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000750.json"),
        r#"{
  "Core": { "Id": "CORE-000750", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"], "Exclude": ["TS"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "USUBJID", "operator": "non_empty" },
      { "name": "--SEQ", "operator": "exists" },
      { "name": "--SEQ", "operator": "is_not_unique_set", "value": ["DOMAIN", "USUBJID"] }
    ]
  },
  "Outcome": {
    "Message": "--SEQ is not unique per subject and domain.",
    "Output Variables": ["USUBJID", "--SEQ", "POOLID"]
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
      "filename": "lbae.xpt",
      "domain": "LBAE",
      "records": {
        "STUDYID": ["S"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [4],
        "LBTESTCD": ["SURVSTAT"]
      }
    },
    {
      "filename": "lbds.xpt",
      "domain": "LBDS",
      "records": {
        "STUDYID": ["S"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [4],
        "LBTESTCD": ["SURVSTAT"]
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
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    let issues = outcome
        .results
        .iter()
        .flat_map(|result| result.errors.iter())
        .map(|issue| {
            (
                issue.dataset.as_str(),
                issue.row,
                issue.usubjid.as_deref(),
                issue.seq.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        issues,
        vec![
            ("LBAE", Some(1), Some("SUBJ1"), Some("4")),
            ("LBDS", Some(1), Some("SUBJ1"), Some("4")),
        ]
    );
}

#[test]
fn run_validation_core_000495_omits_unique_set_group_locator_from_variables() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000495.json"),
        r#"{
  "Core": { "Id": "CORE-000495", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["MA"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "MAORRES", "operator": "non_empty" },
      { "name": "MASPEC", "operator": "non_empty" },
      {
        "name": "MAORRES",
        "operator": "is_not_unique_set",
        "value": ["USUBJID", "MATESTCD", "MASPEC"]
      }
    ]
  },
  "Outcome": {
    "Message": "The Macroscopic Finding test result is not unique for this subject and test.",
    "Output Variables": ["USUBJID", "MATESTCD", "MAORRES", "MASPEC"]
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
      "filename": "ma.xpt",
      "domain": "MA",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["MA", "MA"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "MASEQ": [1, 2],
        "MATESTCD": ["GROSPATH", "GROSPATH"],
        "MAORRES": ["Discoloration dark, mucosal", "Discoloration dark, mucosal"],
        "MASPEC": ["SKIN", "SKIN"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    for issue in &result.errors {
        assert_eq!(issue.variables, vec!["MATESTCD", "MAORRES", "MASPEC"]);
        assert_eq!(issue.usubjid.as_deref(), Some("SUBJ1"));
    }
}

#[test]
fn run_validation_core_000526_includes_subject_locator_in_unique_set_variables() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000526.json"),
        r#"{
  "Core": { "Id": "CORE-000526", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["PP"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      {
        "name": "PPTESTCD",
        "operator": "is_not_unique_set",
        "value": ["USUBJID", "PPCAT", "PPSPEC", "PPTPTREF"]
      },
      {
        "name": "PPTESTCD",
        "operator": "is_not_unique_set",
        "value": ["POOLID", "PPCAT", "PPSPEC", "PPTPTREF"]
      }
    ]
  },
  "Outcome": {
    "Message": "The combination of POOLID or USUBJID, PPTESTCD, PPCAT, PPSPEC, and PPTPTREF is not unique",
    "Output Variables": ["POOLID", "PPTESTCD", "PPCAT", "PPSPEC", "PPTPTREF"]
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
      "filename": "pp.xpt",
      "domain": "PP",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["PP", "PP"],
        "USUBJID": ["123101", "123101"],
        "POOLID": ["", ""],
        "PPSEQ": [1, 2],
        "PPTESTCD": ["AUCINT", "AUCINT"],
        "PPCAT": ["XYZ-123", "XYZ-123"],
        "PPSPEC": ["PLASMA", "PLASMA"],
        "PPTPTREF": ["Day 1 dose", "Day 1 dose"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    for issue in &result.errors {
        assert!(issue.variables.contains(&"USUBJID".to_owned()));
        assert_eq!(issue.usubjid.as_deref(), Some("123101"));
    }
}

#[test]
fn run_validation_core_000516_reports_both_study_day_variables_when_either_is_negative() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000516.json"),
        r#"{
  "Core": { "Id": "CORE-000516", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["EC"] }, "Classes": { "Include": ["INTERVENTIONS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "any": [
      { "name": "--STDY", "operator": "less_than", "value": 0 },
      { "name": "--ENDY", "operator": "less_than", "value": 0 }
    ]
  },
  "Outcome": {
    "Message": "Negative value of Study Day variable.",
    "Output Variables": ["--STDY", "--ENDY"]
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
      "filename": "ec.xpt",
      "domain": "EC",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["EC", "EC"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "ECSEQ": [1, 2],
        "ECSTDY": [-1, 2],
        "ECENDY": [0, -2]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(result.errors[0].variables, vec!["ECSTDY", "ECENDY"]);
    assert_eq!(result.errors[1].row, Some(2));
    assert_eq!(result.errors[1].variables, vec!["ECSTDY", "ECENDY"]);
}

#[test]
fn run_validation_core_000862_reports_dataset_level_existing_study_day_variable_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000862.json"),
        r#"{
  "Core": { "Id": "CORE-000862", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CM"] }, "Classes": { "Include": ["INTERVENTIONS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--STDY", "operator": "exists" },
      { "name": "--STDTC", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "Start Date/Time of Observation (--STDTC) variable is missing when Study Day of Start of Observation (--STDY) is present."
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
        "STUDYID": ["S", "S"],
        "DOMAIN": ["CM", "CM"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "CMSEQ": [1, 2],
        "CMSTDY": [3, 4]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["CMSTDY"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000700_reports_dataset_level_existing_day_variable_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000700.json"),
        r#"{
  "Core": { "Id": "CORE-000700", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CE"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--DY", "operator": "exists" },
      { "name": "--DTC", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "Date/Time of Collection (--DTC) variable is missing when Study Day of Visit/Collection/Exam (--DY) is present."
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
      "filename": "ce.xpt",
      "domain": "CE",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["CE", "CE"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "CESEQ": [1, 2],
        "CEDY": [3, 4]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["CEDY"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000793_reports_dataset_level_existing_date_variable_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000793.json"),
        r#"{
  "Core": { "Id": "CORE-000793", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CE"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "is_complete_date" },
      { "name": "--DY", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "Collection study day (--DY) is missing when date/time of collection (--DTC) is populated."
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
      "filename": "ce.xpt",
      "domain": "CE",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["CE", "CE"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "CESEQ": [1, 2],
        "CEDTC": ["2012-11-23", "2012-11-24"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["CEDTC"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000321_reports_dataset_level_existing_date_variable_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000321.json"),
        r#"{
  "Core": { "Id": "CORE-000321", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CE"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--DY", "operator": "not_exists" },
      { "name": "--DTC", "operator": "exists" }
    ]
  },
  "Outcome": {
    "Message": "Study Day of Visit/Collection/Exam (--DY) variable is missing when Date/Time of Collection (--DTC) is present."
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
      "filename": "ce.xpt",
      "domain": "CE",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["CE", "CE"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "CESEQ": [1, 2],
        "CEDTC": ["", "2012-11-24"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["CEDTC"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000328_reports_dataset_level_existing_start_date_variable_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000328.json"),
        r#"{
  "Core": { "Id": "CORE-000328", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CE"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--STDY", "operator": "not_exists" },
      { "name": "--STDTC", "operator": "exists" }
    ]
  },
  "Outcome": {
    "Message": "The Study Day of Start of Observation (--STDY) is not present in the dataset when Start Date/Time of Observation (--STDTC) is present."
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
      "filename": "ce.xpt",
      "domain": "CE",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["CE", "CE"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "CESEQ": [1, 2],
        "CESTDTC": ["", "2012-11-24"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["CESTDTC"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000864_treats_all_empty_smendy_as_not_present() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000864.json"),
        r#"{
  "Core": { "Id": "CORE-000864", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--ENDY", "operator": "exists" },
      { "name": "--ENDTC", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "End Date/Time of Observation (--ENDTC) variable is missing when Study Day of End of Observation (--ENDY) is present."
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
      "filename": "sm.xpt",
      "domain": "SM",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["SM", "SM"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "SMSEQ": [1, 2],
        "SMENDY": ["", ""]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn run_validation_core_000786_reports_missing_ti_dataset_presence_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000786.json"),
        r#"{
  "Core": { "Id": "CORE-000786", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["TI"] }, "Classes": { "Include": ["TRIAL DESIGN"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "IESCAT", "operator": "exists" },
      { "name": "IECAT", "operator": "not_exists" }
    ]
  },
  "Outcome": {
    "Message": "IESCAT exists in a dataset, but IECAT does not exist.",
    "Output Variables": ["IECAT", "IESCAT"]
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
        "STUDYID": ["S"],
        "DOMAIN": ["AE"],
        "USUBJID": ["SUBJ1"]
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

    let result = &outcome.results[0];
    assert_eq!(result.rule_id, "CORE-000786");
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.dataset, "TI");
    assert_eq!(result.domain.as_deref(), Some("TI"));
    assert_eq!(result.error_count, 2);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["IECAT"]);
    assert_eq!(result.errors[1].row, None);
    assert_eq!(result.errors[1].variables, vec!["IESCAT"]);
    assert!(result.errors.iter().all(|issue| issue.usubjid.is_none()));
    assert!(result.errors.iter().all(|issue| issue.seq.is_none()));
}

#[test]
fn run_validation_core_000853_evaluates_dm_when_dm_is_the_match_dataset() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000853.json"),
        r#"{
  "Core": { "Id": "CORE-000853", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "CM"] }, "Classes": { "Include": ["SPECIAL PURPOSE", "INTERVENTIONS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DM", "Keys": ["USUBJID"] }
  ],
  "Check": {
    "all": [
      { "name": "--DTC", "operator": "non_empty" },
      { "name": "--DTC", "operator": "is_complete_date" },
      { "name": "RFSTDTC", "operator": "non_empty" },
      { "name": "RFSTDTC", "operator": "is_complete_date" },
      { "name": "--DY", "operator": "empty" }
    ]
  },
  "Outcome": {
    "Message": "Collection study day (--DY) is not populated when date/time of collection (--DTC) is populated."
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
        "STUDYID": ["S"],
        "DOMAIN": ["DM"],
        "USUBJID": ["SUBJ1"],
        "RFSTDTC": ["2024-01-01"],
        "DMDTC": ["2024-01-02"],
        "DMDY": [null]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "STUDYID": ["S"],
        "DOMAIN": ["CM"],
        "USUBJID": ["SUBJ1"],
        "CMSEQ": [1],
        "CMDTC": ["2024-01-03"],
        "CMDY": [null]
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

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().any(|result| result.dataset == "DM"));
    assert!(failed.iter().any(|result| result.dataset == "CM"));
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
fn run_validation_core_000597_matches_suppae_aesosp_to_parent_ae_record() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let fail_data_dir = dir.path().join("fail-data");
    let pass_data_dir = dir.path().join("pass-data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&fail_data_dir).expect("fail data dir");
    fs::create_dir_all(&pass_data_dir).expect("pass data dir");

    fs::write(
        rules_dir.join("CORE-000597.json"),
        r#"{
  "Core": { "Id": "CORE-000597", "Status": "Published" },
  "Scope": { "Classes": { "Include": ["EVENTS"] }, "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SUPPAE", "Keys": ["USUBJID"] }
  ],
  "Check": { "all": [
    { "name": "QNAM", "operator": "equal_to", "value": "AESOSP", "value_is_literal": true },
    { "name": "AESMIE", "operator": "not_equal_to", "value": "Y" }
  ] },
  "Outcome": {
    "Message": "Missing AESMIE=Y where SUPPAE.QNAM=AESOSP",
    "Output Variables": ["QNAM", "AESMIE"]
  }
}"#,
    )
    .expect("write CORE-000597 rule");

    let fail_dataset_path = fail_data_dir.join("datasets.json");
    fs::write(
        &fail_dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S", "S", "S"],
        "DOMAIN": ["AE", "AE", "AE"],
        "USUBJID": ["S1", "S1", "S1"],
        "AESEQ": [1, 2, 3],
        "AESMIE": ["Y", "N", "N"]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "STUDYID": ["S"],
        "RDOMAIN": ["AE"],
        "USUBJID": ["S1"],
        "IDVAR": ["AESEQ"],
        "IDVARVAL": ["2"],
        "QNAM": ["AESOSP"],
        "QVAL": ["QUALIFIER"]
      }
    }
  ]
}"#,
    )
    .expect("write failing data");

    let fail_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![fail_dataset_path],
        ..Default::default()
    })
    .expect("run failing validation");

    assert_eq!(fail_outcome.results.len(), 1);
    assert_eq!(
        fail_outcome.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(fail_outcome.results[0].error_count, 1);
    assert_eq!(fail_outcome.results[0].errors[0].row, Some(2));
    assert_eq!(fail_outcome.results[0].errors[0].seq.as_deref(), Some("2"));
    assert_eq!(
        fail_outcome.results[0].errors[0].variables,
        vec!["QNAM", "AESMIE"]
    );

    let pass_dataset_path = pass_data_dir.join("datasets.json");
    fs::write(
        &pass_dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S", "S", "S"],
        "DOMAIN": ["AE", "AE", "AE"],
        "USUBJID": ["S1", "S1", "S1"],
        "AESEQ": [1, 2, 3],
        "AESMIE": ["N", "Y", "N"]
      }
    },
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "STUDYID": ["S"],
        "RDOMAIN": ["AE"],
        "USUBJID": ["S1"],
        "IDVAR": ["AESEQ"],
        "IDVARVAL": ["2"],
        "QNAM": ["AESOSP"],
        "QVAL": ["QUALIFIER"]
      }
    }
  ]
}"#,
    )
    .expect("write passing data");

    let pass_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![pass_dataset_path],
        ..Default::default()
    })
    .expect("run passing validation");

    assert_eq!(pass_outcome.results.len(), 1);
    assert_eq!(
        pass_outcome.results[0].execution_status,
        ExecutionStatus::Passed
    );
    assert_eq!(pass_outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_executes_core_000670_ds_unscheduled_flag_semantics() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000670.json"),
        r#"{
  "Core": { "Id": "CORE-000670", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DD"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "DS", "Keys": ["USUBJID"] }
  ],
  "Check": { "all": [
    { "name": "DDTESTCD", "operator": "non_empty" },
    { "name": "DSDECOD", "operator": "not_equal_to", "value": "ACCIDENTAL DEATH" },
    { "name": "DSDECOD", "operator": "not_equal_to", "value": "FOUND DEAD" }
  ] },
  "Outcome": {
    "Message": "DD record is not linked to an unscheduled death disposition",
    "Output Variables": ["DDTESTCD", "DSDECOD"]
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
      "filename": "dd.xpt",
      "domain": "DD",
      "records": {
        "STUDYID": ["S", "S"],
        "DOMAIN": ["DD", "DD"],
        "USUBJID": ["S1", "S2"],
        "DDSEQ": [1, 1],
        "DDTESTCD": ["DEATHD", "DEATHD"]
      }
    },
    {
      "filename": "ds.xpt",
      "domain": "DS",
      "records": {
        "USUBJID": ["S1", "S2"],
        "DSDECOD": ["ACCIDENTAL DEATH ", "ACCIDENTAL DEATH "],
        "DSUSCHFL": ["Y", ""]
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
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["DDTESTCD", "DSUSCHFL"]
    );
}

#[test]
fn run_validation_core_000884_reports_ts_age_parameter_counts() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let fail_data_dir = dir.path().join("fail-data");
    let pass_data_dir = dir.path().join("pass-data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&fail_data_dir).expect("fail data dir");
    fs::create_dir_all(&pass_data_dir).expect("pass data dir");

    fs::write(
        rules_dir.join("CORE-000884.json"),
        r#"{
  "Core": { "Id": "CORE-000884", "Status": "Published" },
  "Scope": { "Classes": { "Include": ["SPECIAL PURPOSE", "TRIAL DESIGN"] }, "Domains": { "Include": ["DM", "TS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "id": "$ageu_count", "operator": "record_count", "domain": "TS", "name": "TSVAL", "filter": { "TSPARMCD": "AGEU" } }
  ],
  "Check": { "any": [
    { "all": [
      { "name": "DOMAIN", "operator": "equal_to", "value": "DM" },
      { "name": "AGEU", "operator": "empty" },
      { "any": [
        { "name": "AGETXT", "operator": "non_empty" },
        { "name": "AGE", "operator": "non_empty" }
      ] }
    ] },
    { "all": [
      { "name": "DOMAIN", "operator": "equal_to", "value": "TS" },
      { "name": "$ageu_count", "operator": "equal_to", "value": 0 }
    ] }
  ] },
  "Outcome": {
    "Message": "AGE or AGETXT is populated, but AGEU is not populated",
    "Output Variables": ["DOMAIN", "$ageu_count"]
  }
}"#,
    )
    .expect("write rule");

    let fail_dataset_path = fail_data_dir.join("datasets.json");
    fs::write(
        &fail_dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "DOMAIN": ["DM", "DM"],
        "USUBJID": ["S1", "S2"],
        "AGE": [14, null],
        "AGETXT": ["", "20-30"],
        "AGEU": ["YEARS", ""]
      }
    },
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "DOMAIN": ["TS", "TS", "TS"],
        "TSSEQ": [1, 2, 3],
        "TSPARMCD": ["AGE", "AGETXT", "AGEUnitsIsNotAParameter"],
        "TSVAL": ["22", "20-25", "DAYS"]
      }
    }
  ]
}"#,
    )
    .expect("write failing data");

    let fail_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![fail_dataset_path],
        ..Default::default()
    })
    .expect("run failing validation");

    assert_eq!(fail_outcome.results.len(), 1);
    assert_eq!(
        fail_outcome.results[0].execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(fail_outcome.results[0].dataset, "TS");
    assert_eq!(fail_outcome.results[0].error_count, 1);
    assert_eq!(fail_outcome.results[0].errors[0].row, None);
    assert_eq!(
        fail_outcome.results[0].errors[0].variables,
        vec!["$age_count", "DOMAIN", "$ageu_count", "$agetxt_count"]
    );

    let pass_dataset_path = pass_data_dir.join("datasets.json");
    fs::write(
        &pass_dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ts.xpt",
      "domain": "TS",
      "records": {
        "DOMAIN": ["TS", "TS", "TS"],
        "TSSEQ": [1, 2, 3],
        "TSPARMCD": ["AGE", "AGETXT", "AGEU"],
        "TSVAL": ["22", "20-25", "DAYS"]
      }
    }
  ]
}"#,
    )
    .expect("write passing data");

    let pass_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![pass_dataset_path],
        ..Default::default()
    })
    .expect("run passing validation");

    assert_eq!(pass_outcome.results.len(), 1);
    assert_eq!(
        pass_outcome.results[0].execution_status,
        ExecutionStatus::Passed
    );
    assert_eq!(pass_outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_core_000893_reports_one_group_level_distinct_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000893.json"),
        r#"{
  "Core": { "Id": "CORE-000893", "Status": "Published" },
  "Scope": { "Classes": { "Include": ["TRIAL DESIGN"] }, "Domains": { "Include": ["TX"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "id": "$txparmcd", "operator": "distinct", "domain": "TX", "name": "TXPARMCD", "group": ["SETCD"] }
  ],
  "Check": {
    "name": "$txparmcd",
    "operator": "does_not_contain",
    "value": "GRPLBL"
  },
  "Outcome": {
    "Message": "TX dataset should include exactly one TXPARMCD = GRPLBL record per SETCD.",
    "Output Variables": ["$txparmcd"]
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
      "filename": "tx.xpt",
      "domain": "TX",
      "records": {
        "DOMAIN": ["TX", "TX", "TX"],
        "TXSEQ": [1, 2, 3],
        "SETCD": ["A", "A", "A"],
        "TXPARMCD": ["TCNTRL", "ARMCD", "SPGRPCD"]
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
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert_eq!(outcome.results[0].errors[0].variables, vec!["$txparmcd"]);
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
fn run_validation_skips_relrec_and_supp_match_dataset_rules() {
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
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 2);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Skipped));
    assert!(outcome.results.iter().all(|result| matches!(
        result.skipped_reason,
        Some(SkippedReason::OracleSemanticsGap | SkippedReason::DatasetJoinNotSupported)
    )));
}

#[test]
fn run_validation_reports_core_000744_faobj_not_matching_related_ae_term() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000744_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FASEQ": [1],
        "FALNKGRP": ["L1"],
        "FAOBJ": ["WRONG"]
      }
    },
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"],
        "AELNKID": ["L1"],
        "AETERM": ["FATIGUE"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["AE"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["FAOBJ", "RELREC.**TERM", "RELREC.**TRT", "RELREC.**DECOD"]
    );
}

#[test]
fn run_validation_reports_core_000744_ex_parent_variables_with_double_underscore_names() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000744_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FASEQ": [1],
        "FALNKID": ["T1"],
        "FAOBJ": ["WRONG"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "USUBJID": ["S1"],
        "EXLNKID": ["T1"],
        "EXTRT": ["DRUG Z"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["EX"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["FAOBJ", "RELREC.__TERM", "RELREC.__TRT", "RELREC.__DECOD"]
    );
}

#[test]
fn run_validation_core_000744_prefers_specific_relrec_links_over_direct_ids() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000744_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1", "S1"],
        "FASEQ": [1, 2],
        "FASPID": ["1", "2"],
        "FAOBJ": ["WRONG", "WRONG"]
      }
    },
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1", "S1"],
        "AESEQ": [1, 2],
        "AESPID": ["1", "2"],
        "AETERM": ["HEADACHE", "FATIGUE"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["AE", "FA"],
        "USUBJID": ["S1", "S1"],
        "IDVAR": ["AESEQ", "FASEQ"],
        "IDVARVAL": [2, 2],
        "RELID": ["R1", "R1"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(2));
}

fn write_core_000744_rule(rules_dir: &std::path::Path) {
    fs::write(
        rules_dir.join("CORE-000744.yml"),
        r#"
Core:
  Id: CORE-000744
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - FA
Match Datasets:
  - Name: RELREC
Check:
  all:
    - name: FAOBJ
      operator: not_equal_to
      value: RELREC.**TERM
    - name: FAOBJ
      operator: not_equal_to
      value: RELREC.**TRT
    - name: FAOBJ
      operator: not_equal_to_case_insensitive
      value: RELREC.**DECOD
Outcome:
  Message: Related record is present in the parent domain dataset but FAOBJ is not equal to the parent value.
  Output Variables:
    - FAOBJ
    - RELREC.**TERM
    - RELREC.**TRT
    - RELREC.**DECOD
"#,
    )
    .expect("write CORE-000744 rule");
}

#[test]
fn run_validation_reports_core_000757_group_key_relrec_parent_trt_mismatch() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000757_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1", "S1"],
        "FASEQ": [1, 2],
        "FAGRPID": [3, 2],
        "FAOBJ": ["ERYTHEMA", "ASPIRIN"]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S1", "S1"],
        "CMSEQ": [1, 2],
        "CMGRPID": [3, 2],
        "CMTRT": ["HYDROCORTISONE, TOPICAL", "ASPIRIN"],
        "CMDECOD": ["", ""]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["FA", "CM"],
        "IDVAR": ["FAGRPID", "CMGRPID"],
        "IDVARVAL": ["", ""],
        "RELID": ["CMFA-1", "CMFA-1"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].dataset, "CM");
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["CMTRT", "CMDECOD", "RELREC.FAOBJ"]
    );
}

#[test]
fn run_validation_reports_core_000757_explicit_relrec_parent_trt_mismatch() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000757_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FASEQ": [3],
        "FAOBJ": ["ASPIRINA"]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S1"],
        "CMSEQ": [1],
        "CMTRT": ["ASPIRIN"],
        "CMDECOD": [""]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["FA", "CM"],
        "IDVAR": ["FASEQ", "CMSEQ"],
        "IDVARVAL": [3, 1],
        "RELID": ["CMFA-1", "CMFA-1"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].dataset, "CM");
    assert_eq!(result.errors[0].row, Some(1));
}

#[test]
fn run_validation_passes_core_000757_when_parent_decod_is_present() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000757_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "records": {
        "USUBJID": ["S1"],
        "FASEQ": [1],
        "FAGRPID": [1],
        "FAOBJ": ["ERYTHEMA"]
      }
    },
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S1"],
        "CMSEQ": [1],
        "CMGRPID": [1],
        "CMTRT": ["HYDROCORTISONE, TOPICAL"],
        "CMDECOD": ["CORTISONE"]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["FA", "CM"],
        "IDVAR": ["FAGRPID", "CMGRPID"],
        "IDVARVAL": ["", ""],
        "RELID": ["CMFA-1", "CMFA-1"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
}

#[test]
fn run_validation_passes_core_000757_when_fa_parent_dataset_is_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000757_rule(&rules_dir);

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "cm.xpt",
      "domain": "CM",
      "records": {
        "USUBJID": ["S1"],
        "CMSEQ": [1],
        "CMTRT": ["ASPIRIN"],
        "CMDECOD": [""]
      }
    },
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "RDOMAIN": ["CM"],
        "RELID": ["CMFA-1"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
}

fn write_core_000757_rule(rules_dir: &std::path::Path) {
    fs::write(
        rules_dir.join("CORE-000757.yml"),
        r#"
Core:
  Id: CORE-000757
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Scope:
  Classes:
    Include:
      - INTERVENTIONS
Match Datasets:
  - Name: RELREC
    Wildcard: FA
Check:
  all:
    - name: --DECOD
      operator: empty
    - name: RELREC.FAOBJ
      operator: non_empty
    - name: --TRT
      operator: not_equal_to
      value: RELREC.FAOBJ
Outcome:
  Message: Interventions parent record exists and --DECOD = null, but FAOBJ is not equal to --TRT.
  Output Variables:
    - RELREC.FAOBJ
    - --TRT
    - --DECOD
"#,
    )
    .expect("write CORE-000757 rule");
}

#[test]
fn run_validation_does_not_mix_default_missing_column_skips_with_supported_issues() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000481.yml"),
        r#"
Core:
  Id: CORE-000481
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - ALL
Check:
  all:
    - name: --EXCLFL
      operator: not_equal_to
      value: Y
    - name: --REASEX
      operator: non_empty
Outcome:
  Message: --REASEX may only be present when --EXCLFL is 'Y'
  Output Variables:
    - --EXCLFL
    - --REASEX
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "records": {
        "USUBJID": ["S1"],
        "VSSEQ": [3],
        "VSEXCLFL": [""],
        "VSREASEX": ["Reason exclusion"]
      }
    },
    {
      "filename": "sc.xpt",
      "domain": "SC",
      "records": {
        "USUBJID": ["S1"],
        "SCSEQ": [1],
        "SCREASEX": ["Reason exclusion"]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.dataset, "VS");
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].variables, vec!["VSEXCLFL", "VSREASEX"]);
}

#[test]
fn run_validation_narrows_simple_any_issue_variables_to_failing_targets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000558.yml"),
        r#"
Core:
  Id: CORE-000558
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - DS
Check:
  any:
    - name: --DY
      operator: not_matches_regex
      value: ^-?[1-9]{1}\d*$
    - name: VISITDY
      operator: not_matches_regex
      value: ^-?[1-9]{1}\d*$
Outcome:
  Message: Study day variable is not a non-zero integer
  Output Variables:
    - --DY
    - VISITDY
"#,
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
        "USUBJID": ["S1"],
        "DSSEQ": [1],
        "DSDY": [0]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].variables, vec!["DSDY"]);
}

#[test]
fn run_validation_reports_missing_dataset_column_once_for_dataset_presence_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000096.yml"),
        r#"
Core:
  Id: CORE-000096
  Status: Published
Sensitivity: Dataset
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - AE
Check:
  all:
    - name: --LOC
      operator: not_exists
    - name: --PORTOT
      operator: exists
Outcome:
  Message: --PORTOT variable is present, when --LOC variable does not exist in a dataset.
  Output Variables:
    - --LOC
    - --PORTOT
"#,
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
        "USUBJID": ["S1", "S2"],
        "AESEQ": [1, 2],
        "AEPORTOT": ["PARTIAL", "HEMI"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(result.errors[0].variables, vec!["AELOC", "AEPORTOT"]);
    assert_eq!(result.errors[1].row, Some(2));
    assert_eq!(result.errors[1].variables, vec!["AEPORTOT"]);
}

#[test]
fn run_validation_keeps_missing_dataset_column_per_row_for_timepoint_presence_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000171.yml"),
        r#"
Core:
  Id: CORE-000171
  Status: Published
Sensitivity: Dataset
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - CM
Check:
  all:
    - name: --ENRTPT
      operator: exists
    - name: --ENTPT
      operator: not_exists
Outcome:
  Message: --ENTPT should be present when --ENRTPT is present in a dataset
  Output Variables:
    - --ENRTPT
    - --ENTPT
"#,
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
        "USUBJID": ["S1", "S2"],
        "CMSEQ": [1, 2],
        "CMENRTPT": ["", "ONGOING"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(result.errors[0].variables, vec!["CMENRTPT", "CMENTPT"]);
    assert_eq!(result.errors[1].variables, vec!["CMENRTPT", "CMENTPT"]);
}

#[test]
fn run_validation_core_000165_reports_missing_timepoint_reference_column_once() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000165.yml"),
        r#"
Core:
  Id: CORE-000165
  Status: Published
Sensitivity: Dataset
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - VS
Check:
  all:
    - name: --RFTDTC
      operator: exists
    - name: --TPTREF
      operator: not_exists
Outcome:
  Message: TPTREF is missing when RFTDTC is present.
  Output Variables:
    - --RFTDTC
    - --TPTREF
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "records": {
        "STUDYID": ["S", "S", "S"],
        "DOMAIN": ["VS", "VS", "VS"],
        "USUBJID": ["S1", "S1", "S1"],
        "VSSEQ": [1, 2, 3],
        "VSRFTDTC": ["2012-12-01T08:00", "2012-12-01T08:00", "2012-12-01T08:00"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 4);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(result.errors[0].variables, vec!["VSRFTDTC"]);
    assert_eq!(result.errors[1].row, Some(2));
    assert_eq!(result.errors[1].variables, vec!["VSRFTDTC"]);
    assert_eq!(result.errors[2].row, Some(3));
    assert_eq!(result.errors[2].variables, vec!["VSRFTDTC"]);
    assert_eq!(result.errors[3].row, Some(0));
    assert_eq!(result.errors[3].variables, vec!["VSTPTREF"]);
}

#[test]
fn run_validation_reports_first_row_only_for_elapsed_timepoint_presence_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000167.yml"),
        r#"
Core:
  Id: CORE-000167
  Status: Published
Sensitivity: Dataset
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - VS
Check:
  all:
    - name: --ELTM
      operator: exists
    - name: --TPTREF
      operator: not_exists
Outcome:
  Message: --TPTREF must be present when --ELTM is present in a dataset
  Output Variables:
    - --ELTM
    - --TPTREF
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "records": {
        "USUBJID": ["S1", "S2"],
        "VSSEQ": [1, 2],
        "VSELTM": ["PT30M", "PT1H"]
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

    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(result.errors[0].variables, vec!["VSELTM", "VSTPTREF"]);
}

#[test]
fn run_validation_reports_core_000206_idvarval_values_missing_from_rdomain_records() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000206_rule(&rules_dir);
    write_core_000206_open_rules_metadata(&data_dir);
    fs::write(
        data_dir.join("co.csv"),
        "STUDYID,DOMAIN,RDOMAIN,USUBJID,COSEQ,IDVAR,IDVARVAL\nS,CO,LB,S001,1,LBGRPID,20\n",
    )
    .expect("write co csv");
    fs::write(
        data_dir.join("relrec.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,RELTYPE,RELID\nS,LB,S001,LBSEQ,320,ONE,1\nS,AE,S001,AESEQ,2,ONE,2\n",
    )
    .expect("write relrec csv");
    fs::write(
        data_dir.join("supplb.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,QNAM,QVAL\nS,LB,S001,LBSEQ,320,LBCLSIG,Y\n",
    )
    .expect("write supplb csv");
    fs::write(
        data_dir.join("lb.csv"),
        "STUDYID,DOMAIN,USUBJID,LBSEQ,LBGRPID\nS,LB,S001,321,21\nS,LB,S002,320,20\n",
    )
    .expect("write lb csv");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ\nS,AE,S001,1\n",
    )
    .expect("write ae csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 4);
    let issues = result
        .errors
        .iter()
        .map(|issue| (issue.dataset.as_str(), issue.row, issue.usubjid.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        issues,
        vec![
            ("CO", Some(1), Some("S001")),
            ("RELREC", Some(1), Some("S001")),
            ("RELREC", Some(2), Some("S001")),
            ("SUPPLB", Some(1), Some("S001")),
        ]
    );
    assert!(result
        .errors
        .iter()
        .all(|issue| issue.variables == vec!["RDOMAIN", "USUBJID", "IDVAR", "IDVARVAL"]));
}

#[test]
fn run_validation_passes_core_000206_when_idvarval_values_exist_in_rdomain_records() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000206_rule(&rules_dir);
    write_core_000206_open_rules_metadata(&data_dir);
    fs::write(
        data_dir.join("co.csv"),
        "STUDYID,DOMAIN,RDOMAIN,USUBJID,COSEQ,IDVAR,IDVARVAL\nS,CO,LB,S001,1,LBGRPID,20\nS,CO,LB,S002,2,LBSEQ,321\n",
    )
    .expect("write co csv");
    fs::write(
        data_dir.join("relrec.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,RELTYPE,RELID\nS,LB,S001,LBSEQ,320,ONE,1\nS,AE,S001,AESEQ,1,ONE,2\n",
    )
    .expect("write relrec csv");
    fs::write(
        data_dir.join("supplb.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,QNAM,QVAL\nS,LB,S001,LBSEQ,320,LBCLSIG,Y\n",
    )
    .expect("write supplb csv");
    fs::write(
        data_dir.join("lb.csv"),
        "STUDYID,DOMAIN,USUBJID,LBSEQ,LBGRPID\nS,LB,S001,320,20\nS,LB,S002,321,30\n",
    )
    .expect("write lb csv");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ\nS,AE,S001,1\n",
    )
    .expect("write ae csv");

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
fn run_validation_core_000206_supp_rows_follow_domain_level_oracle_boundary() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000206_rule(&rules_dir);
    write_core_000206_open_rules_metadata(&data_dir);
    fs::write(
        data_dir.join("co.csv"),
        "STUDYID,DOMAIN,RDOMAIN,USUBJID,COSEQ,IDVAR,IDVARVAL\n",
    )
    .expect("write co csv");
    fs::write(
        data_dir.join("relrec.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,RELTYPE,RELID\n",
    )
    .expect("write relrec csv");
    fs::write(
        data_dir.join("supplb.csv"),
        "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,QNAM,QVAL\nS,LB,S001,LBSEQ,320,LBCLSIG,Y\nS,LB,S001,LBGRPID,20,LBCLSIG,Y\n",
    )
    .expect("write supplb csv");
    fs::write(
        data_dir.join("lb.csv"),
        "STUDYID,DOMAIN,USUBJID,LBSEQ,LBGRPID\nS,LB,S001,299,21\nS,LB,S002,319,20\n",
    )
    .expect("write lb csv");
    fs::write(data_dir.join("ae.csv"), "STUDYID,DOMAIN,USUBJID,AESEQ\n").expect("write ae csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].dataset, "SUPPLB");
    assert_eq!(result.errors[0].row, Some(2));
}

fn write_core_000206_rule(rules_dir: &std::path::Path) {
    fs::write(
        rules_dir.join("CORE-000206.yml"),
        r#"
Core:
  Id: CORE-000206
  Status: Published
Sensitivity: Record
Rule Type: Record Data
Scope:
  Domains:
    Include:
      - CO
      - SUPP--
      - RELREC
Match Datasets:
  - Child: true
    Keys:
      - USUBJID
      - IDVAR
      - IDVARVAL
    Name: SUPP--
  - Child: true
    Keys:
      - USUBJID
      - IDVAR
      - IDVARVAL
    Name: CO
  - Child: true
    Keys:
      - USUBJID
      - IDVAR
      - IDVARVAL
    Name: RELREC
Check:
  all:
    - name: IDVAR
      operator: non_empty
    - name: IDVARVAL
      operator: non_empty
    - name: IDVARVAL
      operator: not_equal_to
      type_insensitive: true
      value: IDVAR
      value_is_reference: true
Outcome:
  Message: IDVARVAL does not equal a value of the variable referenced by IDVAR in domain = RDOMAIN.
  Output Variables:
    - RDOMAIN
    - USUBJID
    - IDVAR
    - IDVARVAL
"#,
    )
    .expect("write CORE-000206 rule");
}

fn write_core_000206_open_rules_metadata(data_dir: &std::path::Path) {
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nco,Comments\nrelrec,Related Records\nsupplb,Supplemental Qualifiers for LB\nlb,Laboratory Test Results\nae,Adverse Events\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nco,STUDYID,Study Identifier,Char,12\nco,DOMAIN,Domain Abbreviation,Char,2\nco,RDOMAIN,Related Domain,Char,2\nco,USUBJID,Unique Subject Identifier,Char,8\nco,COSEQ,Sequence Number,Num,8\nco,IDVAR,Identifying Variable,Char,8\nco,IDVARVAL,Identifying Variable Value,Char,8\nrelrec,STUDYID,Study Identifier,Char,12\nrelrec,RDOMAIN,Related Domain,Char,2\nrelrec,USUBJID,Unique Subject Identifier,Char,8\nrelrec,IDVAR,Identifying Variable,Char,8\nrelrec,IDVARVAL,Identifying Variable Value,Char,8\nrelrec,RELTYPE,Relationship Type,Char,8\nrelrec,RELID,Relationship Identifier,Char,8\nsupplb,STUDYID,Study Identifier,Char,12\nsupplb,RDOMAIN,Related Domain,Char,2\nsupplb,USUBJID,Unique Subject Identifier,Char,8\nsupplb,IDVAR,Identifying Variable,Char,8\nsupplb,IDVARVAL,Identifying Variable Value,Char,8\nsupplb,QNAM,Qualifier Variable Name,Char,8\nsupplb,QVAL,Data Value,Char,8\nlb,STUDYID,Study Identifier,Char,12\nlb,DOMAIN,Domain Abbreviation,Char,2\nlb,USUBJID,Unique Subject Identifier,Char,8\nlb,LBSEQ,Sequence Number,Num,8\nlb,LBGRPID,Group ID,Char,8\nae,STUDYID,Study Identifier,Char,12\nae,DOMAIN,Domain Abbreviation,Char,2\nae,USUBJID,Unique Subject Identifier,Char,8\nae,AESEQ,Sequence Number,Num,8\n",
    )
    .expect("write variables csv");
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
