use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
fn run_validation_reports_core_000454_rfxendtc_when_outcome_lists_rfxstdtc() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000454.json"),
        r#"{
  "Core": { "Id": "CORE-000454", "Status": "Published" },
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
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exendtc" },
      { "name": "RFXENDTC", "operator": "not_equal_to", "value": "$max_ex_exstdtc" }
    ]
  },
  "Outcome": {
    "Message": "RFXSTDTC does not equal the latest value of EX.EXENDTC or the latest value of EX.EXSTDTC if EX.EXENDTC is not present or not populated.",
    "Output Variables": ["RFXSTDTC", "$max_ex_exendtc", "$max_ex_exstdtc"]
  }
}"#,
    )
    .expect("write CORE-000454 rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "RFXSTDTC": ["2020-01-01"],
        "RFXENDTC": ["2020-01-04"]
      }
    },
    {
      "filename": "ex.xpt",
      "domain": "EX",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "EXSTDTC": ["2020-01-03", "2020-01-05"],
        "EXENDTC": ["2020-01-03", "2020-01-05"]
      }
    }
  ]
}"#,
    )
    .expect("write CORE-000454 data");

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
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["RFXENDTC", "$max_ex_exendtc", "$max_ex_exstdtc"]
    );
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
        open_rules_oracle_compat: true,
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
