use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
