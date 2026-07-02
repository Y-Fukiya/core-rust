use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
fn run_validation_executes_core_000690_label_uniqueness_direction() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000690.json"),
        r#"{
  "Core": { "Id": "CORE-000690", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Check": { "name": "variable_name", "operator": "is_not_unique_relationship", "value": "variable_label" },
  "Outcome": {
    "Message": "Variable label is not unique for each variable in the dataset.",
    "Output Variables": ["variable_label", "variable_name"]
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
      "filename": "gt.xpt",
      "domain": "GT",
      "variables": [
        { "name": "GTREFID", "label": "Reference ID", "type": "Char", "length": 8 },
        { "name": "GTREFID", "label": "Lab Test or Examination Short Name", "type": "Char", "length": 50 }
      ],
      "records": { "GTREFID": ["A"] }
    },
    {
      "filename": "relref.xpt",
      "domain": "RELREF",
      "variables": [
        { "name": "LEVEL", "label": "Reference ID Generation Level", "type": "Num", "length": 8 },
        { "name": "LVLDESC", "label": "Reference ID Generation Level", "type": "Char", "length": 50 }
      ],
      "records": { "LEVEL": [1], "LVLDESC": ["Level 1"] }
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
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert_eq!(failed[0].dataset, "RELREF");
    assert_eq!(failed[0].error_count, 2);
    assert_eq!(
        failed[0]
            .errors
            .iter()
            .filter_map(|error| error.row)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
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
        "rel_type": ["definition", "definition", "definition", "definition", "definition", "label"],
        "name": ["VALID", "BAD_TAG", "BAD_XML", "BAD_LIST", "BAD_REF", "IGNORED"],
        "text": [
          "<p>At least <usdm:tag name=\"min_age\"/> years.</p>",
          "<p><usdm:tag nam=\"min_age\"/></p>",
          "Insulin-dependent & diabetic",
          "<div><ul><li><p>Allowed item</p></li><ul></ul></ul></div>",
          "<p><usdm:ref attribute=\"text\" klass=\"StudyTitle\"/></p>",
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
    assert_eq!(outcome.results[0].error_count, 4);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[1].row, Some(3));
    assert_eq!(outcome.results[0].errors[2].row, Some(4));
    assert_eq!(outcome.results[0].errors[3].row, Some(5));
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
fn run_validation_joins_single_match_dataset_with_prefixed_condition_column() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000249.json"),
        r#"{
  "Core": { "Id": "CORE-000249", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "TV", "Keys": ["VISITNUM"] }
  ],
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
      { "name": "VISITDY", "operator": "exists" },
      { "name": "VISITNUM", "operator": "is_contained_by", "value": "$tv_visitnum" },
      { "name": "VISITDY", "operator": "not_equal_to", "value": "TV.VISITDY" }
    ]
  },
  "Outcome": {
    "Message": "Visit Day cannot be found in Trial Visit (TV) domain",
    "Output Variables": ["VISITDY", "VISITNUM"]
  }
}"#,
    )
    .expect("write visitdy rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["LB", "LB"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "LBSEQ": [1, 2],
        "VISITNUM": ["100", "200"],
        "VISITDY": ["-14", "-2"]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["TV", "TV"],
        "VISITNUM": ["100", "200"],
        "VISITDY": ["-14", "1"]
      }
    }
  ]
}"#,
    )
    .expect("write visitdy data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1, "{result:?}");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(result.errors[0].seq.as_deref(), Some("2"));
    assert_eq!(
        result.errors[0].variables,
        vec!["VISITDY".to_owned(), "VISITNUM".to_owned()]
    );
}

#[test]
fn run_validation_keeps_unprefixed_match_columns_for_prefixed_conflicts() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000784.json"),
        r#"{
  "Core": { "Id": "CORE-000784", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SV"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SE", "Keys": ["USUBJID"] }
  ],
  "Check": { "all": [
    { "name": "TAETORD", "operator": "exists" },
    { "name": "SVSTDTC", "operator": "date_greater_than_or_equal_to", "value": "SESTDTC" },
    { "name": "SVSTDTC", "operator": "date_less_than_or_equal_to", "value": "SEENDTC" },
    { "name": "TAETORD", "operator": "not_equal_to", "value": "SE.TAETORD" }
  ] },
  "Outcome": {
    "Message": "SV TAETORD does not match the active SE element",
    "Output Variables": ["SESTDTC", "SEENDTC", "SVSTDTC", "TAETORD", "SE.TAETORD"]
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
      "filename": "sv.xpt",
      "domain": "SV",
      "records": {
        "STUDYID": ["S1", "S1"],
        "DOMAIN": ["SV", "SV"],
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "VISITNUM": [1, 2],
        "SVSTDTC": ["2020-01-05", "2020-01-06"],
        "TAETORD": [1, 9]
      }
    },
    {
      "filename": "se.xpt",
      "domain": "SE",
      "records": {
        "USUBJID": ["SUBJ1", "SUBJ1"],
        "SESTDTC": ["2020-01-01", "2020-02-01"],
        "SEENDTC": ["2020-01-31", "2020-02-28"],
        "TAETORD": [1, 2]
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
    assert_eq!(result.error_count, 1, "{result:?}");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(
        result.errors[0].variables,
        vec![
            "SESTDTC".to_owned(),
            "SEENDTC".to_owned(),
            "SVSTDTC".to_owned(),
            "TAETORD".to_owned(),
            "SE.TAETORD".to_owned(),
        ]
    );
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
