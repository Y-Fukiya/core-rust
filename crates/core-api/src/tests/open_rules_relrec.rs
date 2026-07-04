use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
