use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::helpers::write_dataset;
use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_core_000390_uses_overlapping_evaluation_interval_uniqueness() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000390.json"),
        r#"{
  "Core": { "Id": "CORE-000390", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["CV"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "name": "CVTESTCD",
    "operator": "is_not_unique_set",
    "value": ["USUBJID", "CVDTC"]
  },
  "Outcome": {
    "Message": "The cardiovascular test is not unique for this subject and measurement datetime.",
    "Output Variables": ["USUBJID", "CVTESTCD", "CVDTC"]
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
      "filename": "cv.xpt",
      "domain": "CV",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
        "CVSEQ": [1, 2, 1, 2],
        "CVTESTCD": ["SYSBP", "SYSBP", "DIABP", "DIABP"],
        "CVDTC": ["2024-01-01", "2024-01-01", "2024-01-01", "2024-01-01"],
        "CVTPTREF": ["", "", "Dose 1", "Dose 1"],
        "CVSTINT": ["", "", "-PT60M", "PT60M"],
        "CVENINT": ["", "", "PT0M", "PT120M"]
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
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0]
            .errors
            .iter()
            .filter_map(|issue| issue.row)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
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
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].skipped_reason, None);
    assert_eq!(outcome.results[0].error_count, 3);
}

#[test]
fn run_validation_core_000142_scopes_elapsed_time_consistency_to_precondition_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000142.json"),
        r#"{
  "Core": { "Id": "CORE-000142", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["FT"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "--TPT", "operator": "non_empty" },
      { "name": "--TPTNUM", "operator": "non_empty" },
      { "name": "--ELTM", "operator": "non_empty" },
      {
        "name": "--ELTM",
        "operator": "is_inconsistent_across_dataset",
        "value": ["DOMAIN", "VISITNUM", "--TPTREF", "--TPTNUM"]
      }
    ]
  },
  "Outcome": {
    "Message": "--ELTM is not the same value across records with the same values of DOMAIN, VISITNUM, --TPTREF, and --TPTNUM.",
    "Output Variables": ["--TPT", "--TPTNUM", "--ELTM", "VISITNUM", "DOMAIN"]
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
      "filename": "ft.xpt",
      "domain": "FT",
      "records": {
        "DOMAIN": ["FT", "FT", "FT", "FT"],
        "FTSEQ": [1, 2, 3, 4],
        "VISITNUM": [1, 1, 2, 2],
        "FTTPTREF": ["DOSE", "DOSE", "BREAKFAST", "BREAKFAST"],
        "FTTPTNUM": [1, 1, 2, 2],
        "FTTPT": ["30 MIN POST", "30 MIN POST", "90 MIN POST", ""],
        "FTELTM": ["PT30M", "PT03M", "PT1H", "PT2H"]
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
    assert_eq!(outcome.results[0].error_count, 2);
    assert_eq!(
        outcome.results[0]
            .errors
            .iter()
            .filter_map(|issue| issue.row)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    for issue in &outcome.results[0].errors {
        assert_eq!(issue.variables, vec!["FTTPT", "FTTPTNUM", "FTELTM"]);
    }
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
