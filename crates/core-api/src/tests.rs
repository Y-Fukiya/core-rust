use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;

mod basic_validation;
mod helpers;
mod open_rules_codelists;
mod open_rules_data_loader;
mod open_rules_dataset_presence;
mod open_rules_dates;
mod open_rules_entities;
mod open_rules_idvarval;
mod open_rules_joins_basic;
mod open_rules_joins_oracle_gaps;
mod open_rules_joins_timeline;
mod open_rules_joins_usdm;
mod open_rules_jsonata;
mod open_rules_match_datasets;
mod open_rules_metadata_dataset;
mod open_rules_metadata_domain;
mod open_rules_metadata_library;
mod open_rules_metadata_variable;
mod open_rules_operations;
mod open_rules_operations_distinct;
mod open_rules_operations_dy;
mod open_rules_operations_match_dataset;
mod open_rules_operations_metadata;
mod open_rules_operations_min_max;
mod open_rules_operations_pipeline;
mod open_rules_operations_record_count;
mod open_rules_operator_basics;
mod open_rules_oracle_semantics;
mod open_rules_reference_distinct;
mod open_rules_relrec;
mod open_rules_scope;
mod open_rules_standard_filter;
mod open_rules_unique_set;
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
        vec!["DOMAIN", "$ageu_count"]
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
