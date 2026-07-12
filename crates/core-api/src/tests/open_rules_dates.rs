use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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

    let default_outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path.clone()],
        ..Default::default()
    })
    .expect("run default validation");

    assert_eq!(default_outcome.results.len(), 1);
    assert_eq!(
        default_outcome.results[0].execution_status,
        ExecutionStatus::Passed
    );
    assert_eq!(default_outcome.results[0].error_count, 0);

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
