use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};
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
fn run_validation_does_not_require_usubjid_for_trial_design_required_variables() {
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
      "filename": "ta.xpt",
      "domain": "TA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "ARMCD", "label": "Planned Arm Code", "type": "Char", "length": 20 },
        { "name": "ARM", "label": "Description of Planned Arm", "type": "Char", "length": 200 },
        { "name": "TAETORD", "label": "Planned Order of Element within Arm", "type": "Num", "length": 8 },
        { "name": "ETCD", "label": "Element Code", "type": "Char", "length": 8 },
        { "name": "ELEMENT", "label": "Description of Element", "type": "Char", "length": 200 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["TA"],
        "ARMCD": ["A"],
        "ARM": ["Active"],
        "TAETORD": [1],
        "ETCD": ["E1"],
        "ELEMENT": ["Treatment"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000355.json"),
        r#"{
  "Core": { "Id": "CORE-000355", "Status": "Published" },
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
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
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
