use std::{collections::BTreeSet, fs};

use core_engine::ExecutionStatus;
use core_rule_model::{load_rules_from_paths, Sensitivity};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;
use helpers::{write_dataset, write_raw_rule, write_rule, write_test_xpt_char_dataset};

mod basic_validation;
mod helpers;
mod open_rules_data_loader;
mod open_rules_dates;
mod open_rules_usdm;

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
fn run_validation_scans_forbidden_send_domain_placeholder_variables_across_datasets() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        rules_dir.join("CORE-000794.json"),
        r#"{
  "Core": { "Id": "CORE-000794", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["EVENTS"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "--PTCD", "operator": "exists" },
  "Outcome": { "Message": "--PTCD must not be present" }
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
      "variables": [
        { "name": "STUDYID" },
        { "name": "DMPTCD" }
      ],
      "records": { "STUDYID": ["S1"], "DMPTCD": ["1"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID" },
        { "name": "VSPTCD" }
      ],
      "records": { "STUDYID": ["S1"], "VSPTCD": ["1"] }
    },
    {
      "filename": "tx.xpt",
      "domain": "TX",
      "variables": [
        { "name": "STUDYID" },
        { "name": "TXSEQ" }
      ],
      "records": { "STUDYID": ["S1"], "TXSEQ": [1] }
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
    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();

    assert_eq!(failed.len(), 2, "{failed:#?}");
    assert_eq!(failed[0].dataset, "DM");
    assert_eq!(failed[0].errors[0].row, None);
    assert_eq!(failed[0].errors[0].variables, vec!["DMPTCD"]);
    assert_eq!(failed[1].dataset, "VS");
    assert_eq!(failed[1].errors[0].row, None);
    assert_eq!(failed[1].errors[0].variables, vec!["VSPTCD"]);
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        "VISITNUM": ["1", "2"]
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
fn run_validation_executes_grouped_reference_distinct_without_aliases() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000168.json"),
        r#"{
  "Core": { "Id": "CORE-000168", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["SV"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "SV", "group": ["USUBJID"], "id": "$sv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "VISITNUM", "operator": "non_empty" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$sv_visitnum" }
    ]
  },
  "Outcome": {
    "Message": "VISITNUM should be among subject-level SV.VISITNUM values.",
    "Output Variables": ["VISITNUM"]
  }
}"#,
    )
    .expect("write grouped reference distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S1", "S2"],
        "VISITNUM": ["1", "1.01", "2"],
        "SVPRESP": ["Y", "", "Y"]
      }
    },
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1", "S1", "S2"],
        "LBSEQ": [1, 2, 3],
        "VISITNUM": ["1.01", "3", "2"]
      }
    },
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": ["3"]
      }
    }
  ]
}"#,
    )
    .expect("write grouped reference distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let lb = outcome
        .results
        .iter()
        .find(|result| result.dataset == "LB")
        .expect("LB result");
    assert_eq!(lb.execution_status, ExecutionStatus::Failed);
    assert_eq!(lb.skipped_reason, None);
    assert_eq!(lb.error_count, 1, "{:?}", lb.errors);
    assert_eq!(lb.errors[0].row, Some(2));

    assert!(
        outcome.results.iter().all(|result| result.dataset != "TV"),
        "datasets without the grouped reference key are not applicable"
    );
}

#[test]
fn run_validation_reference_distinct_keeps_decimal_source_values_from_open_rules_csv() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000168.json"),
        r#"{
  "Core": { "Id": "CORE-000168", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["SV"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    { "domain": "SV", "group": ["USUBJID"], "id": "$sv_visitnum", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "VISITNUM", "operator": "non_empty" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$sv_visitnum" }
    ]
  },
  "Outcome": {
    "Message": "VISITNUM should be among subject-level SV.VISITNUM values.",
    "Output Variables": ["VISITNUM"]
  }
}"#,
    )
    .expect("write grouped reference distinct rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nsv,Subject Visits\nlb,Laboratory Test Results\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nSV,USUBJID,Unique Subject Identifier,Char,20\nSV,SVSEQ,Sequence Number,Num,8\nSV,VISITNUM,Visit Number,Num,8\nSV,SVPRESP,Planned Visit Flag,Char,1\nLB,USUBJID,Unique Subject Identifier,Char,20\nLB,LBSEQ,Sequence Number,Num,8\nLB,VISITNUM,Visit Number,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("sv.csv"),
        "USUBJID,SVSEQ,VISITNUM,SVPRESP\nCDISC008,25,1.01,\n",
    )
    .expect("write sv csv");
    fs::write(
        data_dir.join("lb.csv"),
        "USUBJID,LBSEQ,VISITNUM\nCDISC008,652,1.01\nCDISC008,665,99\n",
    )
    .expect("write lb csv");

    let loaded = core_data::load_open_rules_data_dir(&data_dir).expect("load fixture data");
    let sv = loaded
        .iter()
        .find(|dataset| dataset.metadata.name == "SV")
        .expect("SV dataset");
    assert_eq!(
        core_data::dataset_column_values(sv, "VISITNUM").expect("SV VISITNUM"),
        vec![serde_json::json!(1.01)]
    );

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![data_dir.clone()],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let lb = outcome
        .results
        .iter()
        .find(|result| result.dataset == "LB")
        .expect("LB result");
    assert_eq!(lb.execution_status, ExecutionStatus::Failed);
    assert_eq!(lb.skipped_reason, None);
    assert_eq!(lb.error_count, 1, "{:?}", lb.errors);
    assert_eq!(lb.errors[0].row, Some(2), "{:?}", lb.errors);
}

#[test]
fn run_validation_joins_match_dataset_before_reference_distinct_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000270.json"),
        r#"{
  "Core": { "Id": "CORE-000270", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["TV"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SV", "Keys": ["USUBJID", "VISITNUM"] }
  ],
  "Operations": [
    { "domain": "TV", "id": "$TV_VISITNUM", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$TV_VISITNUM" }
    ]
  },
  "Outcome": {
    "Message": "Planned VISITNUM should be among TV.VISITNUM.",
    "Output Variables": ["SVPRESP", "VISITNUM"]
  }
}"#,
    )
    .expect("write match dataset reference distinct rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": ["1"]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S2"],
        "VISITNUM": ["2", "1"],
        "SVPRESP": ["Y", "Y"]
      }
    },
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1", "S2"],
        "LBSEQ": [1, 2],
        "VISITNUM": ["2", "1"]
      }
    },
    {
      "filename": "ae.csv",
      "domain": "AE",
      "records": {
        "USUBJID": ["S1"],
        "AESEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write match dataset reference distinct data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir.clone()],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    let lb = outcome
        .results
        .iter()
        .find(|result| result.dataset == "LB")
        .expect("LB result");
    assert_eq!(lb.execution_status, ExecutionStatus::Failed);
    assert_eq!(lb.skipped_reason, None);
    assert_eq!(lb.error_count, 1);
    assert_eq!(lb.errors[0].row, Some(1));
}

#[test]
fn run_validation_evaluates_planned_visit_match_dataset_as_target() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000270.json"),
        r#"{
  "Core": { "Id": "CORE-000270", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["TV"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SV", "Keys": ["USUBJID", "VISITNUM"] }
  ],
  "Operations": [
    { "domain": "TV", "id": "$TV_VISITNUM", "name": "VISITNUM", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISITNUM", "operator": "is_not_contained_by", "value": "$TV_VISITNUM" }
    ]
  },
  "Outcome": {
    "Message": "Planned VISITNUM should be among TV.VISITNUM.",
    "Output Variables": ["SVPRESP", "VISITNUM"]
  }
}"#,
    )
    .expect("write planned visitnum rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISITNUM": ["1"]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1", "S1"],
        "VISITNUM": ["1", "2"],
        "SVPRESP": ["Y", "Y"]
      }
    },
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1"],
        "LBSEQ": [1],
        "VISITNUM": ["1"]
      }
    }
  ]
}"#,
    )
    .expect("write planned visitnum data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    let result = outcome
        .results
        .iter()
        .find(|result| result.dataset == "SV")
        .expect("SV result");
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(
        result.errors[0].variables,
        vec!["SVPRESP".to_owned(), "VISITNUM".to_owned()]
    );
}

#[test]
fn run_validation_core_000269_evaluates_sv_match_dataset_as_target() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000269.json"),
        r#"{
  "Core": { "Id": "CORE-000269", "Status": "Published" },
  "Scope": { "Domains": { "Exclude": ["TV"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    { "Name": "SV", "Keys": ["USUBJID", "VISIT"] }
  ],
  "Operations": [
    { "domain": "TV", "id": "$tv_visit", "name": "VISIT", "operator": "distinct" }
  ],
  "Check": {
    "all": [
      { "name": "VISIT", "operator": "non_empty" },
      { "name": "SVPRESP", "operator": "equal_to", "value": "Y" },
      { "name": "VISIT", "operator": "is_not_contained_by", "value": "$tv_visit" }
    ]
  },
  "Outcome": {
    "Message": "Planned VISIT should be among TV.VISIT.",
    "Output Variables": ["VISIT", "VISITNUM"]
  }
}"#,
    )
    .expect("write planned visit rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "tv.csv",
      "domain": "TV",
      "records": {
        "VISIT": ["WEEK 24"]
      }
    },
    {
      "filename": "sv.csv",
      "domain": "SV",
      "records": {
        "USUBJID": ["S1"],
        "VISIT": ["WEEK 26"],
        "VISITNUM": ["13"],
        "SVPRESP": ["Y"]
      }
    },
    {
      "filename": "lb.csv",
      "domain": "LB",
      "records": {
        "USUBJID": ["S1"],
        "LBSEQ": [1],
        "VISIT": ["WEEK 26"],
        "VISITNUM": ["13"]
      }
    }
  ]
}"#,
    )
    .expect("write planned visit data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let mut datasets = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .map(|result| result.dataset.as_str())
        .collect::<Vec<_>>();
    datasets.sort_unstable();
    assert_eq!(datasets, vec!["LB", "SV"]);
}

#[test]
fn run_validation_core_000878_reports_all_invalid_condition_context_ids() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000878.json"),
        r#"{
  "Core": { "Id": "CORE-000878", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["ALL"], "Exclude": ["Activity", "ScheduledActivityInstance"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": {
    "all": [
      { "name": "parent_rel", "operator": "equal_to", "value": "contextIds", "value_is_literal": true },
      { "name": "$condition_count", "operator": "non_empty" }
    ]
  },
  "Operations": [
    { "domain": "Condition", "filter": { "rel_type": "definition" }, "group": ["id", "instanceType"], "group_aliases": ["parent_id", "parent_entity"], "id": "$condition_count", "operator": "record_count" },
    { "domain": "Condition", "filter": { "rel_type": "definition" }, "group": ["id", "instanceType"], "group_aliases": ["parent_id", "parent_entity"], "id": "$condition_parent_entity", "name": "parent_entity", "operator": "distinct" }
  ],
  "Outcome": {
    "Message": "Invalid condition context.",
    "Output Variables": ["$condition_parent_entity", "$condition_parent_id", "$condition_parent_rel", "$condition_rel_type", "$condition_name", "id", "name", "parent_id", "parent_rel", "rel_type", "instanceType", "value", "$error_type"]
  }
}"#,
    )
    .expect("write condition context rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "condition.csv",
      "domain": "Condition",
      "records": {
        "parent_entity": ["StudyDesign", "Condition"],
        "parent_id": ["StudyDesign_1", "Condition_2"],
        "parent_rel": ["conditions", "contextIds"],
        "rel_type": ["definition", "reference"],
        "id": ["Condition_1", "Condition_1"],
        "name": ["COND1", "COND1"],
        "instanceType": ["Condition", "Condition"]
      }
    },
    {
      "filename": "biomedicalconcept.csv",
      "domain": "BiomedicalConcept",
      "records": {
        "parent_entity": ["Condition"],
        "parent_id": ["Condition_1"],
        "parent_rel": ["contextIds"],
        "rel_type": ["reference"],
        "id": ["BiomedicalConcept_1"],
        "name": ["Heart Rate"],
        "instanceType": ["BiomedicalConcept"]
      }
    },
    {
      "filename": "string.csv",
      "domain": "string",
      "records": {
        "parent_entity": ["Condition"],
        "parent_id": ["Condition_1"],
        "parent_rel": ["contextIds"],
        "rel_type": ["definition"],
        "value": ["Activity_missing"]
      }
    }
  ]
}"#,
    )
    .expect("write condition context data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let issue_count = outcome
        .results
        .iter()
        .flat_map(|result| result.errors.iter())
        .count();
    assert_eq!(issue_count, 3, "{:?}", outcome.results);
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
        rules_dir.join("CORE-000017.json"),
        r#"{
  "Core": { "Id": "CORE-000017", "Status": "Published" },
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
        open_rules_oracle_compat: true,
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
fn run_validation_reports_core_000677_poolid_values_missing_from_pooldef() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pooldef.csv",
      "domain": "POOLDEF",
      "records": {
        "POOLID": ["POOL1", "POOL2"]
      }
    },
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "DOMAIN": ["VS", "VS", "VS"],
        "POOLID": ["POOL1", "POOL3", ""],
        "VSSEQ": [1, 2, 3]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].dataset, "VS");
    assert_eq!(result.errors[0].row, Some(2));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_passes_core_000677_when_pooldef_is_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
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
        "DOMAIN": ["VS"],
        "POOLID": ["POOL3"],
        "VSSEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(outcome.results[0].error_count, 0);
}

#[test]
fn run_validation_reports_core_000677_open_rules_data_dir_poolid_mismatches() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000677.yml"),
        r#"
Core:
  Id: CORE-000677
  Status: Published
Sensitivity: Dataset
Rule Type: Domain Presence Check
Operations:
  - id: $poolid_exists
    name: POOLID
    operator: variable_exists
Check:
  all:
    - name: $poolid_exists
      operator: equal_to
      value: true
    - name: POOLDEF
      operator: not_exists
Outcome:
  Message: POOLID value in the dataset does not correspond to a POOLID value in POOLDEF.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nvs,Vital Signs\npooldef,Pool Definition\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nvs,STUDYID,Study Identifier,Char,12\nvs,DOMAIN,Domain Abbreviation,Char,2\nvs,USUBJID,Unique Subject Identifier,Char,8\nvs,POOLID,Pool Identifier,Char,8\nvs,VSSEQ,Sequence Number,Num,8\npooldef,STUDYID,Study Identifier,Char,12\npooldef,POOLID,Pool Identifier,Char,8\npooldef,USUBJID,Unique Subject Identifier,Char,8\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("pooldef.csv"),
        "STUDYID,POOLID,USUBJID\nCDISCPILOT01,POOL1,\nCDISCPILOT01,POOL2,\n",
    )
    .expect("write pooldef csv");
    fs::write(
        data_dir.join("vs.csv"),
        "STUDYID,DOMAIN,USUBJID,POOLID,VSSEQ\nCDISCPILOT01,VS,SUBJ1,POOL4,1\nCDISCPILOT01,VS,SUBJ2,POOL2,2\n",
    )
    .expect("write vs csv");

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
    assert_eq!(result.dataset, "VS");
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_reports_core_000896_poolid_values_missing_from_pooldef() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000896.yml"),
        r#"
Core:
  Id: CORE-000896
  Status: Published
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
  Message: POOLID value does not match a POOLID value in the POOLDEF dataset.
  Output Variables:
    - $pooldef_poolid
    - POOLID
"#,
    )
    .expect("write rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "pooldef.csv",
      "domain": "POOLDEF",
      "records": {
        "POOLID": ["POOL1", "POOL2", "POOL3"]
      }
    },
    {
      "filename": "vs.csv",
      "domain": "VS",
      "records": {
        "STUDYID": ["S1", "S1", "S1", "S1", "S1", "S1", "S1"],
        "DOMAIN": ["VS", "VS", "VS", "VS", "VS", "VS", "VS"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3", "SUBJ4", "SUBJ5", "SUBJ6", "SUBJ7"],
        "POOLID": ["POOL1", "POOL1", "POOL1", "POOL3", "POOL2", "POOL2", "POOL99"],
        "VSSEQ": [1, 2, 3, 4, 6, 7, 8]
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.dataset, "VS");
    assert_eq!(result.errors[0].row, Some(7));
    assert_eq!(
        result.errors[0].variables,
        vec!["$pooldef_poolid", "POOLID"]
    );
}

#[test]
fn run_validation_keeps_record_scoped_dataset_presence_when_oracle_expects_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ,AESTAT\n\
CDISC-TEST,AE,SUBJ1,1,NOT DONE\n\
CDISC-TEST,AE,SUBJ2,2,\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000013.json"),
        r#"{
  "Core": { "Id": "CORE-000013", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "AESTAT", "operator": "exists" },
  "Outcome": {
    "Message": "AESTAT variable is present in AE dataset.",
    "Output Variables": ["AESTAT"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("ae.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 2);
    assert_eq!(
        result
            .errors
            .iter()
            .map(|issue| issue.row)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2)]
    );
}

#[test]
fn run_validation_collapses_dataset_level_presence_oracle_family() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("ae.csv"),
        "STUDYID,DOMAIN,USUBJID,AESEQ,AEOCCUR\n\
CDISC-TEST,AE,SUBJ1,1,Y\n\
CDISC-TEST,AE,SUBJ2,2,Y\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000012.json"),
        r#"{
  "Core": { "Id": "CORE-000012", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["AE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "AEOCCUR", "operator": "exists" },
  "Outcome": {
    "Message": "AEOCCUR is present in AE dataset.",
    "Output Variables": ["AEOCCUR"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("ae.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["AEOCCUR"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
}

#[test]
fn run_validation_core_000638_reports_dataset_level_forbidden_send_variable() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    fs::write(
        data_dir.join("dm.csv"),
        "STUDYID,DOMAIN,USUBJID,DTHDTC\n\
CDISC-TEST,DM,SUBJ1,\n\
CDISC-TEST,DM,SUBJ2,\n\
CDISC-TEST,DM,SUBJ3,\n",
    )
    .expect("write data");
    fs::write(
        rules_dir.join("CORE-000638.json"),
        r#"{
  "Core": { "Id": "CORE-000638", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Check": { "name": "DTHDTC", "operator": "exists" },
  "Outcome": {
    "Message": "DTHDTC must not be present in SEND dataset",
    "Output Variables": ["DTHDTC"]
  }
}"#,
    )
    .expect("write rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir.join("dm.csv")],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["DTHDTC"]);
    assert_eq!(result.errors[0].usubjid, None);
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
        open_rules_oracle_compat: true,
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
fn run_validation_core_000540_treats_alphanumeric_fa_split_dataset_names_as_candidates() {
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
      "filename": "fa.csv",
      "domain": "FA",
      "label": "Findings About",
      "records": { "DOMAIN": ["FA"] }
    },
    {
      "filename": "facm.csv",
      "domain": "FACM",
      "label": "Findings About CM Records",
      "records": { "DOMAIN": ["FA"] }
    },
    {
      "filename": "fa1.csv",
      "domain": "FA1",
      "label": "Findings About Medical History",
      "records": { "DOMAIN": ["FA"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000540.json"),
        r#"{
  "Core": { "Id": "CORE-000540", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"], "include_split_datasets": true }, "Classes": {} },
  "Sensitivity": "Dataset",
  "Rule Type": "Dataset Metadata Check",
  "Operations": [{ "id": "$list_dataset_names", "operator": "dataset_names" }],
  "Check": {
    "all": [
      { "name": "dataset_name", "operator": "matches_regex", "value": "^[a-z(?-i)]{2}[a-z(?-i)]{1,2}" },
      { "name": "dataset_name", "operator": "prefix_equal_to", "prefix": 2, "value": "FA" },
      { "name": "dataset_name", "operator": "suffix_is_not_contained_by", "suffix": 2, "value": "$list_dataset_names" }
    ]
  },
  "Outcome": {
    "Message": "Parent domain referenced in Findings About dataset name is not present in the study",
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
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();

    assert_eq!(failed.len(), 2, "{failed:#?}");
    assert_eq!(failed[0].errors[0].dataset, "FACM");
    assert_eq!(failed[1].errors[0].dataset, "FA1");
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
    <CodeListItem CodedValue="ZB"><Alias Context="nci:ExtCodeID" Name="C49592"/></CodeListItem>
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
        open_rules_oracle_compat: true,
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

    let sponsor_2024 =
        static_codelist_term_by_code("C188724", &codelist, "C70793", Some("2024-09-27"))
            .expect("2024 clinical study sponsor");
    assert_eq!(sponsor_2024.value, "Clinical Study Sponsor");
    assert_eq!(sponsor_2024.pref_term, "Clinical Study Sponsor");

    let sponsor_2025 =
        static_codelist_term_by_code("C188724", &codelist, "C70793", Some("2025-09-26"))
            .expect("2025 clinical study sponsor");
    assert_eq!(sponsor_2025.value, "Study Sponsor");
    assert_eq!(sponsor_2025.pref_term, "Clinical Study Sponsor");

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
fn ddf_valid_codelist_dates_include_ddf_package_versions() {
    let operation = OperationSpec {
        fields: std::collections::BTreeMap::from([(
            "ct_package_types".to_owned(),
            serde_json::Value::Array(vec![serde_json::Value::String("DDF".to_owned())]),
        )]),
    };

    let dates = valid_codelist_dates_for_operation(&operation);

    assert!(dates.contains(&"2025-09-26"));
    assert!(dates.contains(&"2024-09-27"));
    assert!(dates.contains(&"2023-12-15"));
}

#[test]
fn ddf_study_role_terms_are_scoped_by_package_version() {
    let term = static_codelist("C215480")
        .expect("study role codelist")
        .find_by_code("C78726")
        .expect("adjudication committee");

    assert!(!static_codelist_term_matches_version(
        "C215480",
        term,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C215480",
        term,
        Some("2025-09-26")
    ));
}

#[test]
fn ddf_study_role_codelist_is_scoped_by_package_version() {
    assert!(!static_codelist_matches_version(
        "C215480",
        Some("2024-09-27")
    ));
    assert!(static_codelist_matches_version(
        "C215480",
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_trial_type_terms() {
    let codelist = static_codelist("C66739").expect("trial type codelist");
    assert!(codelist.extensible);

    let alcohol_effect = codelist
        .find_by_code("C158284")
        .expect("alcohol effect term");
    assert_eq!(alcohol_effect.value, "ALCOHOL EFFECT");
    assert_eq!(alcohol_effect.pref_term, "Alcohol Effect Study");

    let water_effect = codelist
        .find_by_value("WATER EFFECT")
        .expect("water effect submission value");
    assert_eq!(water_effect.code, "C161480");
    assert_eq!(water_effect.pref_term, "Water Effect Trial");

    let dose_response = codelist
        .find_by_pref_term("Dose Response Study")
        .expect("dose response preferred term");
    assert_eq!(dose_response.code, "C127803");
    assert_eq!(dose_response.value, "DOSE RESPONSE");
}

#[test]
fn static_codelist_resolves_sdtm_trial_intent_type_terms() {
    let codelist = static_codelist("C66736").expect("trial intent type codelist");
    assert!(codelist.extensible);

    let basic = codelist.find_by_code("C15714").expect("basic science");
    assert_eq!(basic.value, "BASIC SCIENCE");
    assert_eq!(basic.pref_term, "Basic Research");

    let mitigation = codelist
        .find_by_value("MITIGATION")
        .expect("mitigation submission value");
    assert_eq!(mitigation.code, "C49655");
    assert_eq!(mitigation.pref_term, "Adverse Effect Mitigation Study");

    let supportive = codelist
        .find_by_pref_term("Supportive Care Study")
        .expect("supportive care preferred term");
    assert_eq!(supportive.code, "C71486");
    assert_eq!(supportive.value, "SUPPORTIVE CARE");
}

#[test]
fn static_codelist_resolves_sdtm_blinding_schema_terms() {
    let codelist = static_codelist("C66735").expect("blinding schema codelist");
    assert!(codelist.extensible);

    let double_blind = codelist.find_by_code("C15228").expect("double blind");
    assert_eq!(double_blind.value, "DOUBLE BLIND");
    assert_eq!(double_blind.pref_term, "Double Blind Study");

    let open_label = codelist
        .find_by_value("OPEN LABEL")
        .expect("open label submission value");
    assert_eq!(open_label.code, "C49659");
    assert_eq!(open_label.pref_term, "Open Label Study");

    let single_blind = codelist
        .find_by_pref_term("Single Blind Study")
        .expect("single blind");
    assert_eq!(single_blind.code, "C28233");
    assert_eq!(single_blind.value, "SINGLE BLIND");
}

#[test]
fn sdtm_blinding_schema_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C66735").expect("blinding schema codelist");
    let double_blind = codelist.find_by_code("C15228").expect("double blind");
    let single_blind = codelist.find_by_code("C28233").expect("single blind");

    assert!(static_codelist_term_matches_version(
        "C66735",
        double_blind,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C66735",
        single_blind,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C66735",
        single_blind,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_intervention_model_terms() {
    let codelist = static_codelist("C99076").expect("intervention model codelist");
    assert!(codelist.extensible);

    let crossover = codelist.find_by_code("C82637").expect("crossover");
    assert_eq!(crossover.value, "CROSS-OVER");
    assert_eq!(crossover.pref_term, "Crossover Study");

    let parallel = codelist
        .find_by_value("PARALLEL")
        .expect("parallel submission value");
    assert_eq!(parallel.code, "C82639");
    assert_eq!(parallel.pref_term, "Parallel Study");

    let sequential = codelist
        .find_by_pref_term("Group Sequential Design")
        .expect("sequential preferred term");
    assert_eq!(sequential.code, "C142568");
    assert_eq!(sequential.value, "SEQUENTIAL");
}

#[test]
fn sdtm_intervention_model_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C99076").expect("intervention model codelist");
    let crossover = codelist.find_by_code("C82637").expect("crossover");
    let single_group = codelist.find_by_code("C82640").expect("single group");

    assert!(static_codelist_term_matches_version(
        "C99076",
        crossover,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99076",
        single_group,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C99076",
        single_group,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_study_type_terms() {
    let codelist = static_codelist("C99077").expect("study type codelist");
    assert!(!codelist.extensible);

    let interventional = codelist.find_by_code("C98388").expect("interventional");
    assert_eq!(interventional.value, "INTERVENTIONAL");
    assert_eq!(interventional.pref_term, "Interventional Study");

    let expanded_access = codelist
        .find_by_value("EXPANDED ACCESS")
        .expect("expanded access submission value");
    assert_eq!(expanded_access.code, "C98722");
    assert_eq!(expanded_access.pref_term, "Expanded Access Study");

    let patient_registry = codelist
        .find_by_pref_term("Patient Registry Study")
        .expect("patient registry preferred term");
    assert_eq!(patient_registry.code, "C129000");
    assert_eq!(patient_registry.value, "PATIENT REGISTRY");
}

#[test]
fn static_codelist_resolves_sdtm_route_terms() {
    let codelist = static_codelist("C66729").expect("route codelist");
    assert!(codelist.extensible);

    let oral = codelist.find_by_code("C38288").expect("oral");
    assert_eq!(oral.value, "ORAL");
    assert_eq!(oral.pref_term, "Oral Route of Administration");

    let transdermal = codelist
        .find_by_value("TRANSDERMAL")
        .expect("transdermal submission value");
    assert_eq!(transdermal.code, "C38305");
    assert_eq!(transdermal.pref_term, "Transdermal Route of Administration");

    let nasoduodenal = codelist
        .find_by_pref_term("Nasoduodenal Route of Administration")
        .expect("nasoduodenal preferred term");
    assert_eq!(nasoduodenal.code, "C188189");
    assert_eq!(nasoduodenal.value, "NASODUODENAL");
}

#[test]
fn static_codelist_resolves_sdtm_frequency_terms() {
    let codelist = static_codelist("C71113").expect("frequency codelist");
    assert!(codelist.extensible);

    let every_eighteen_hours = codelist.find_by_code("C64508").expect("q18h");
    assert_eq!(every_eighteen_hours.value, "Q18H");
    assert_eq!(every_eighteen_hours.pref_term, "Every Eighteen Hours");

    let every_other_day = codelist
        .find_by_value("QOD")
        .expect("every other day submission value");
    assert_eq!(every_other_day.code, "C64525");
    assert_eq!(every_other_day.pref_term, "Every Other Day");

    let three_times_weekly = codelist
        .find_by_pref_term("Three Times Weekly")
        .expect("three times weekly preferred term");
    assert_eq!(three_times_weekly.code, "C64528");
    assert_eq!(three_times_weekly.value, "3 TIMES PER WEEK");
}

#[test]
fn static_codelist_resolves_ddf_protocol_status_terms() {
    let codelist = static_codelist("C188723").expect("protocol status codelist");
    assert!(!codelist.extensible);

    let approved = codelist.find_by_code("C25425").expect("approved");
    assert_eq!(approved.value, "Approval");
    assert_eq!(approved.pref_term, "Approved");

    let final_status = codelist
        .find_by_value("Final")
        .expect("final submission value");
    assert_eq!(final_status.code, "C25508");
    assert_eq!(final_status.pref_term, "Final");

    let pending_review = codelist
        .find_by_pref_term("Pending Review")
        .expect("pending review preferred term");
    assert_eq!(pending_review.code, "C188862");
    assert_eq!(pending_review.value, "Pending Review");
}

#[test]
fn static_codelist_resolves_ddf_product_designation_terms_by_version() {
    let codelist = static_codelist("C207418").expect("product designation codelist");
    assert!(!codelist.extensible);

    let investigational =
        static_codelist_term_by_code("C207418", &codelist, "C202579", Some("2024-09-27"))
            .expect("investigational product");
    assert_eq!(investigational.value, "IMP");
    assert_eq!(
        investigational.pref_term,
        "Investigational Medicinal Product"
    );

    let auxiliary_2024 =
        static_codelist_term_by_code("C207418", &codelist, "C156473", Some("2024-09-27"))
            .expect("2024 auxiliary product");
    assert_eq!(auxiliary_2024.value, "NIMP (AxMP)");
    assert_eq!(auxiliary_2024.pref_term, "Auxiliary Medicinal Product");

    let auxiliary_2025 =
        static_codelist_term_by_code("C207418", &codelist, "C156473", Some("2025-09-26"))
            .expect("2025 auxiliary product");
    assert_eq!(auxiliary_2025.value, "NIMP");
    assert_eq!(auxiliary_2025.pref_term, "Auxiliary Medicinal Product");

    assert!(
        static_codelist_term_by_value("C207418", &codelist, "NIMP", Some("2024-09-27"),).is_none()
    );
    assert_eq!(
        static_codelist_term_by_value("C207418", &codelist, "NIMP", Some("2025-09-26"))
            .expect("2025 NIMP value")
            .code,
        "C156473"
    );
}

#[test]
fn static_codelist_resolves_sdtm_trial_phase_terms_by_version() {
    let codelist = static_codelist("C66737").expect("trial phase codelist");
    assert!(codelist.extensible);

    let phase_i_ii_iii_2022 =
        static_codelist_term_by_code("C66737", &codelist, "C198366", Some("2022-12-16"))
            .expect("2022 phase I/II/III");
    assert_eq!(phase_i_ii_iii_2022.value, "PHASE I/II/III STUDY");
    assert_eq!(phase_i_ii_iii_2022.pref_term, "Phase I/II/III Study");

    let phase_i_ii_iii_2023 =
        static_codelist_term_by_code("C66737", &codelist, "C198366", Some("2023-12-15"))
            .expect("2023 phase I/II/III");
    assert_eq!(phase_i_ii_iii_2023.value, "PHASE I/II/III TRIAL");
    assert_eq!(phase_i_ii_iii_2023.pref_term, "Phase I/II/III Trial");

    let early_phase_2023 =
        static_codelist_term_by_code("C66737", &codelist, "C54721", Some("2023-12-15"))
            .expect("2023 early phase");
    assert_eq!(early_phase_2023.value, "PHASE 0 TRIAL");

    let early_phase_2024 =
        static_codelist_term_by_code("C66737", &codelist, "C54721", Some("2024-09-27"))
            .expect("2024 early phase");
    assert_eq!(early_phase_2024.value, "EARLY PHASE I");
    assert_eq!(early_phase_2024.pref_term, "Early Phase 1 Trial");

    assert!(static_codelist_term_by_value(
        "C66737",
        &codelist,
        "PHASE 0 TRIAL",
        Some("2025-09-26"),
    )
    .is_none());
}

#[test]
fn static_codelist_resolves_small_oracle_value_sets() {
    let objective = static_codelist("C188725").expect("objective level codelist");
    assert!(!objective.extensible);
    assert_eq!(
        objective
            .find_by_code("C85826")
            .expect("primary objective")
            .value,
        "Study Primary Objective"
    );
    assert_eq!(
        objective
            .find_by_value("Exploratory Objective")
            .expect("exploratory objective")
            .pref_term,
        "Trial Exploratory Objective"
    );

    let endpoint = static_codelist("C188726").expect("endpoint level codelist");
    assert!(!endpoint.extensible);
    assert_eq!(
        endpoint
            .find_by_code("C94496")
            .expect("primary endpoint")
            .value,
        "Primary Endpoint"
    );
    assert_eq!(
        endpoint
            .find_by_pref_term("Exploratory Endpoint")
            .expect("exploratory endpoint")
            .code,
        "C170559"
    );

    let geographic_scope = static_codelist("C207412").expect("geographic scope codelist");
    assert!(!geographic_scope.extensible);
    assert_eq!(
        geographic_scope
            .find_by_code("C25464")
            .expect("country")
            .value,
        "Country"
    );
    assert_eq!(
        geographic_scope
            .find_by_value("Global")
            .expect("global")
            .code,
        "C68846"
    );

    let eligibility_category = static_codelist("C66797").expect("eligibility category codelist");
    assert!(!eligibility_category.extensible);
    assert_eq!(
        eligibility_category
            .find_by_value("EXCLUSION")
            .expect("exclusion")
            .pref_term,
        "Exclusion Criteria"
    );

    let encounter_type = static_codelist("C188728").expect("encounter type codelist");
    assert!(encounter_type.extensible);
    assert_eq!(
        encounter_type.find_by_code("C25716").expect("visit").value,
        "Visit"
    );
}

#[test]
fn static_codelist_resolves_additional_oracle_value_sets() {
    let sampling = static_codelist("C127260").expect("sampling method codelist");
    assert!(sampling.extensible);
    assert!(!static_codelist_matches_version(
        "C127260",
        Some("2016-03-25")
    ));
    assert!(static_codelist_matches_version(
        "C127260",
        Some("2024-09-27")
    ));
    assert_eq!(
        sampling
            .find_by_value("NON-PROBABILITY SAMPLE")
            .expect("non-probability sample")
            .pref_term,
        "Non-Probability Sampling Method"
    );

    let perspective = static_codelist("C127261").expect("time perspective codelist");
    assert!(perspective.extensible);
    assert!(!static_codelist_matches_version(
        "C127261",
        Some("2016-03-25")
    ));
    assert!(static_codelist_matches_version(
        "C127261",
        Some("2024-09-27")
    ));
    assert_eq!(
        perspective
            .find_by_value("RETROSPECTIVE")
            .expect("retrospective")
            .pref_term,
        "Retrospective Study"
    );

    let timing_type = static_codelist("C201264").expect("timing type codelist");
    assert!(!timing_type.extensible);
    assert_eq!(
        timing_type
            .find_by_pref_term("Fixed Reference Timing Type")
            .expect("fixed reference")
            .value,
        "Fixed Reference"
    );

    let governance_date = static_codelist("C207413").expect("governance date codelist");
    assert!(governance_date.extensible);
    assert_eq!(
        governance_date
            .find_by_value("Sponsor Approval Date")
            .expect("sponsor approval")
            .pref_term,
        "Protocol Approval by Sponsor Date"
    );

    let title_type = static_codelist("C207419").expect("study title type codelist");
    assert!(!title_type.extensible);
    assert_eq!(
        title_type
            .find_by_pref_term("Scientific Study Title")
            .expect("scientific title")
            .code,
        "C207618"
    );

    let definition_document =
        static_codelist("C215477").expect("study definition document type codelist");
    assert!(definition_document.extensible);
    assert_eq!(
        definition_document
            .find_by_value("Protocol")
            .expect("protocol")
            .pref_term,
        "Study Protocol"
    );

    let reference_identifier =
        static_codelist("C215478").expect("reference identifier type codelist");
    assert!(reference_identifier.extensible);
    assert_eq!(
        reference_identifier
            .find_by_pref_term("Pediatric Investigation Plan")
            .expect("pediatric investigation plan")
            .value,
        "Pediatric Investigation Clinical Development Plan"
    );

    let product_property = static_codelist("C215479").expect("product property type codelist");
    assert!(product_property.extensible);
    assert_eq!(
        product_property.find_by_code("C45997").expect("ph").value,
        "pH"
    );

    let amendment_impact = static_codelist("C215481").expect("amendment impact codelist");
    assert!(amendment_impact.extensible);
    assert_eq!(
        amendment_impact
            .find_by_value("Study Data Robustness")
            .expect("robustness")
            .code,
        "C215668"
    );

    let medical_device_sourcing =
        static_codelist("C215482").expect("medical device sourcing codelist");
    assert!(medical_device_sourcing.extensible);
    assert_eq!(
        medical_device_sourcing
            .find_by_value("Locally Sourced")
            .expect("locally sourced")
            .pref_term,
        "Locally Sourced Indicator"
    );
    let product_sourcing = static_codelist("C215483").expect("product sourcing codelist");
    assert!(product_sourcing.extensible);
    assert_eq!(
        product_sourcing
            .find_by_pref_term("Centrally Sourced Indicator")
            .expect("centrally sourced")
            .value,
        "Centrally Sourced"
    );

    let device_identifier = static_codelist("C215484").expect("device identifier type codelist");
    assert!(device_identifier.extensible);
    assert_eq!(
        device_identifier
            .find_by_value("FDA Unique Device Identification")
            .expect("fda udi")
            .pref_term,
        "FDA Unique Device Identifier"
    );

    let dosage_form = static_codelist("C66726").expect("dosage form codelist");
    assert!(dosage_form.extensible);
    assert_eq!(
        dosage_form
            .find_by_value("TABLET")
            .expect("tablet")
            .pref_term,
        "Tablet Dosage Form"
    );

    let timing_relative = static_codelist("C201265").expect("timing relative codelist");
    assert!(!timing_relative.extensible);
    assert_eq!(
        timing_relative
            .find_by_value("End to Start")
            .expect("end to start")
            .code,
        "C201353"
    );

    let masking_role = static_codelist("C207414").expect("masking role codelist");
    assert!(masking_role.extensible);
    assert!(static_codelist_matches_version(
        "C207414",
        Some("2024-09-27")
    ));
    assert!(!static_codelist_matches_version(
        "C207414",
        Some("2025-09-26")
    ));
    assert_eq!(
        masking_role
            .find_by_pref_term("Clinical Study Sponsor")
            .expect("clinical sponsor")
            .value,
        "Sponsor"
    );

    let data_origin = static_codelist("C188727").expect("data origin type codelist");
    assert!(data_origin.extensible);
    assert_eq!(
        data_origin
            .find_by_pref_term("Synthetic Data")
            .expect("synthetic data")
            .code,
        "C176263"
    );
    let real_world_2024 =
        static_codelist_term_by_code("C188727", &data_origin, "C165830", Some("2024-09-27"))
            .expect("2024 real world data");
    assert_eq!(real_world_2024.value, "Real World Data");
    let real_world_2025 =
        static_codelist_term_by_code("C188727", &data_origin, "C165830", Some("2025-09-26"))
            .expect("2025 real-world data");
    assert_eq!(real_world_2025.value, "Real-world Data");
}

#[test]
fn static_codelist_resolves_sdtm_environmental_setting_terms() {
    let codelist = static_codelist("C127262").expect("environmental setting codelist");
    assert!(codelist.extensible);

    let childcare = codelist
        .find_by_code("C127785")
        .expect("childcare center term");
    assert_eq!(childcare.value, "CHILD CARE CENTER");
    assert_eq!(childcare.pref_term, "Childcare Center");

    let outpatient = codelist
        .find_by_value("OUTPATIENT CLINIC")
        .expect("outpatient clinic submission value");
    assert_eq!(outpatient.code, "C16281");
    assert_eq!(outpatient.pref_term, "Ambulatory Care Facility");

    let correctional = codelist
        .find_by_pref_term("Correctional Institution")
        .expect("correctional institution preferred term");
    assert_eq!(correctional.code, "C85862");
    assert_eq!(correctional.value, "PRISON");
}

#[test]
fn static_codelist_resolves_sdtm_contact_mode_terms() {
    let codelist = static_codelist("C171445").expect("contact mode codelist");
    assert!(codelist.extensible);

    let email = codelist.find_by_code("C25170").expect("email");
    assert_eq!(email.value, "E-MAIL");
    assert_eq!(email.pref_term, "E-mail");

    let remote_audio_video = codelist
        .find_by_value("REMOTE AUDIO VIDEO")
        .expect("remote audio video submission value");
    assert_eq!(remote_audio_video.code, "C171525");
    assert_eq!(remote_audio_video.pref_term, "Audio-Videoconferencing");

    let ivrs = codelist
        .find_by_pref_term("Interactive Voice Response System")
        .expect("ivrs preferred term");
    assert_eq!(ivrs.code, "C177933");
    assert_eq!(ivrs.value, "IVRS");
}

#[test]
fn static_codelist_resolves_sdtm_age_unit_terms() {
    let codelist = static_codelist("C66781").expect("age unit codelist");
    assert!(!codelist.extensible);

    let hour = codelist.find_by_code("C25529").expect("hour");
    assert_eq!(hour.value, "HOURS");
    assert_eq!(hour.pref_term, "Hour");

    let year = codelist
        .find_by_value("YEARS")
        .expect("years submission value");
    assert_eq!(year.code, "C29848");
    assert_eq!(year.pref_term, "Year");

    let month = codelist.find_by_pref_term("Month").expect("month");
    assert_eq!(month.code, "C29846");
    assert_eq!(month.value, "MONTHS");
}

#[test]
fn static_codelist_resolves_sdtm_unit_terms() {
    let codelist = static_codelist("C71620").expect("unit codelist");
    assert!(codelist.extensible);

    let day = codelist.find_by_code("C25301").expect("day");
    assert_eq!(day.value, "DAYS");
    assert_eq!(day.pref_term, "Day");

    let milligram = codelist.find_by_value("mg").expect("milligram");
    assert_eq!(milligram.code, "C28253");
    assert_eq!(milligram.pref_term, "Milligram");

    let microvolt_second = codelist
        .find_by_pref_term("Microvolt Second")
        .expect("microvolt second");
    assert_eq!(microvolt_second.code, "C105499");
    assert_eq!(microvolt_second.value, "uV*s");
}

#[test]
fn sdtm_unit_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C71620").expect("unit codelist");
    let day = codelist.find_by_code("C25301").expect("day");
    let microvolt_second = codelist.find_by_code("C105499").expect("microvolt second");
    let per_day = codelist.find_by_code("C176378").expect("per day");

    assert!(static_codelist_term_matches_version(
        "C71620",
        day,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C71620",
        microvolt_second,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C71620",
        microvolt_second,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C71620",
        per_day,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C71620",
        per_day,
        Some("2025-09-26")
    ));
}

#[test]
fn sdtm_contact_mode_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C171445").expect("contact mode codelist");
    let email = codelist.find_by_code("C25170").expect("email");
    let ivrs = codelist.find_by_code("C177933").expect("ivrs");

    assert!(static_codelist_term_matches_version(
        "C171445",
        email,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C171445",
        ivrs,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C171445",
        ivrs,
        Some("2024-03-29")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_intervention_type_terms() {
    let codelist = static_codelist("C99078").expect("intervention type codelist");
    assert!(!codelist.extensible);

    let behavioral = codelist.find_by_code("C15184").expect("behavioral");
    assert_eq!(behavioral.value, "BEHAVIORAL THERAPY");
    assert_eq!(behavioral.pref_term, "Behavioral Intervention");

    let device = codelist
        .find_by_value("DEVICE")
        .expect("device submission value");
    assert_eq!(device.code, "C16830");
    assert_eq!(device.pref_term, "Medical Device");

    let procedure = codelist
        .find_by_pref_term("Physical Medical Procedure")
        .expect("procedure preferred term");
    assert_eq!(procedure.code, "C98769");
    assert_eq!(procedure.value, "PROCEDURE");
}

#[test]
fn sdtm_intervention_type_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C99078").expect("intervention type codelist");
    let combination = codelist
        .find_by_code("C54696")
        .expect("combination product");
    let non_surgical = codelist
        .find_by_code("C218507")
        .expect("non-surgical procedure");
    let other = codelist.find_by_code("C17649").expect("other");

    assert!(!static_codelist_term_matches_version(
        "C99078",
        combination,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        combination,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99078",
        non_surgical,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        non_surgical,
        Some("2025-09-26")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        other,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99078",
        other,
        Some("2025-03-28")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_observational_model_terms() {
    let codelist = static_codelist("C127259").expect("observational model codelist");
    assert!(codelist.extensible);

    let case_control = codelist.find_by_code("C15197").expect("case control");
    assert_eq!(case_control.value, "CASE CONTROL");
    assert_eq!(case_control.pref_term, "Case-Control Study");

    let cohort = codelist
        .find_by_value("COHORT")
        .expect("cohort submission value");
    assert_eq!(cohort.code, "C15208");
    assert_eq!(cohort.pref_term, "Cohort Study");

    let family = codelist.find_by_pref_term("Family Study").expect("family");
    assert_eq!(family.code, "C15407");
    assert_eq!(family.value, "FAMILY BASED");
}

#[test]
fn sdtm_observational_model_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C127259").expect("observational model codelist");
    let case_control = codelist.find_by_code("C15197").expect("case control");
    let cohort = codelist.find_by_code("C15208").expect("cohort");
    let ecologic = codelist.find_by_code("C127780").expect("ecologic");

    assert!(!static_codelist_matches_version(
        "C127259",
        Some("2016-03-25")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        case_control,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C127259",
        cohort,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        cohort,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C127259",
        ecologic,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        ecologic,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_role_terms() {
    let codelist = static_codelist("C215480").expect("study role codelist");
    assert!(codelist.extensible);

    let care_provider = codelist.find_by_code("C17445").expect("care provider term");
    assert_eq!(care_provider.value, "Care Provider");
    assert_eq!(care_provider.pref_term, "Caregiver");

    let co_sponsor = codelist
        .find_by_value("Co-Sponsor")
        .expect("co-sponsor submission value");
    assert_eq!(co_sponsor.code, "C215669");
    assert_eq!(co_sponsor.pref_term, "Study Co-Sponsor");

    let clinical_sponsor = codelist
        .find_by_pref_term("Clinical Study Sponsor")
        .expect("clinical study sponsor preferred term");
    assert_eq!(clinical_sponsor.code, "C70793");
    assert_eq!(clinical_sponsor.value, "Sponsor");
}

#[test]
fn static_codelist_resolves_ddf_study_amendment_reason_terms() {
    let codelist = static_codelist("C207415").expect("study amendment reason codelist");
    assert!(!codelist.extensible);

    let standard_of_care = codelist
        .find_by_code("C207600")
        .expect("change in standard of care");
    assert_eq!(standard_of_care.value, "Change In Standard Of Care");
    assert_eq!(standard_of_care.pref_term, "Change In Standard Of Care");

    let other = codelist
        .find_by_value("OTHER")
        .expect("other submission value");
    assert_eq!(other.code, "C17649");
    assert_eq!(other.pref_term, "Other");

    let extension = codelist
        .find_by_pref_term("Extension")
        .expect("extension preferred term");
    assert_eq!(extension.code, "C0031X");
    assert_eq!(extension.value, "Extension");
}

#[test]
fn ddf_study_amendment_reason_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207415").expect("study amendment reason codelist");
    let standard_of_care = codelist
        .find_by_code("C207600")
        .expect("change in standard of care");
    let other = codelist.find_by_code("C17649").expect("other");

    assert!(static_codelist_term_matches_version(
        "C207415",
        standard_of_care,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207415",
        other,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207415",
        other,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_design_characteristic_terms() {
    let codelist = static_codelist("C207416").expect("study design characteristic codelist");
    assert!(codelist.extensible);

    let randomized = codelist.find_by_code("C46079").expect("randomized");
    assert_eq!(randomized.value, "Randomized");
    assert_eq!(randomized.pref_term, "Randomized Controlled Clinical Trial");

    let single_centre = codelist
        .find_by_value("Single-Centre")
        .expect("single-centre submission value");
    assert_eq!(single_centre.code, "C217004");
    assert_eq!(single_centre.pref_term, "Single-Center Study");

    let stratified = codelist
        .find_by_pref_term("Stratified Randomization")
        .expect("stratified randomization preferred term");
    assert_eq!(stratified.code, "C147145");
    assert_eq!(stratified.value, "Stratified Randomisation");
}

#[test]
fn ddf_study_design_characteristic_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207416").expect("study design characteristic codelist");
    let randomized = codelist.find_by_code("C46079").expect("randomized");
    let single_centre = codelist.find_by_code("C217004").expect("single-centre");

    assert!(!static_codelist_matches_version(
        "C207416",
        Some("2023-12-15")
    ));
    assert!(static_codelist_matches_version(
        "C207416",
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207416",
        randomized,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207416",
        single_centre,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207416",
        single_centre,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_intervention_role_terms() {
    let codelist = static_codelist("C207417").expect("study intervention role codelist");
    assert!(!codelist.extensible);

    let required = codelist
        .find_by_code("C207614")
        .expect("additional required treatment");
    assert_eq!(required.value, "Additional Required Treatment");
    assert_eq!(required.pref_term, "Additional Required Medicinal Product");

    let diagnostic = codelist
        .find_by_value("Diagnostic")
        .expect("diagnostic submission value");
    assert_eq!(diagnostic.code, "C18020");
    assert_eq!(diagnostic.pref_term, "Diagnostic Procedure");

    let rescue = codelist
        .find_by_pref_term("Rescue Medications")
        .expect("rescue preferred term");
    assert_eq!(rescue.code, "C165835");
    assert_eq!(rescue.value, "Rescue Medicine");
}

#[test]
fn ddf_study_intervention_role_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207417").expect("study intervention role codelist");
    let placebo = codelist.find_by_code("C753").expect("placebo");
    let active = codelist.find_by_code("C68609").expect("active comparator");

    assert!(static_codelist_term_matches_version(
        "C207417",
        placebo,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207417",
        active,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207417",
        active,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_observational_study_subtype_terms() {
    let codelist = static_codelist("C215486").expect("observational subtype codelist");
    assert!(codelist.extensible);

    let education = codelist
        .find_by_code("C215657")
        .expect("clinical education");
    assert_eq!(education.value, "Clinical Education");
    assert_eq!(education.pref_term, "Clinical Education Study");

    let prevalence = codelist
        .find_by_value("Disease Prevalence")
        .expect("disease prevalence submission value");
    assert_eq!(prevalence.code, "C215675");
    assert_eq!(prevalence.pref_term, "Disease Prevalence Study");

    let safety = codelist
        .find_by_pref_term("Safety Study")
        .expect("safety preferred term");
    assert_eq!(safety.code, "C49667");
    assert_eq!(safety.value, "Safety");
}

#[test]
fn ddf_observational_study_subtype_codelist_is_scoped_by_package_version() {
    let term = static_codelist("C215486")
        .expect("observational subtype codelist")
        .find_by_code("C215657")
        .expect("clinical education");

    assert!(!static_codelist_matches_version(
        "C215486",
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C215486",
        term,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C215486",
        term,
        Some("2025-09-26")
    ));
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
fn run_validation_joins_single_match_dataset_with_suffix_condition_column() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-SINGLE-MATCH-SUFFIX.json"),
        r#"{
  "Core": { "Id": "CORE-SINGLE-MATCH-SUFFIX", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "parent_id", "Right": "id" }
      ]
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "rel_type.StudyArm", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id.StudyArm", "operator": "is_not_contained_by", "value": "StudyDesign_2" }
    ]
  },
  "Outcome": {
    "Message": "Population and arm parents must match",
    "Output Variables": ["parent_id", "parent_id.StudyArm", "rel_type.StudyArm"]
  }
}"#,
    )
    .expect("write suffix match dataset rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_id": ["StudyArm_1"],
        "parent_rel": ["populationIds"],
        "rel_type": ["reference"],
        "id": ["StudyDesignPopulation_1"],
        "name": ["POP1"],
        "instanceType": ["StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["arms"],
        "rel_type": ["definition"],
        "id": ["StudyArm_1"],
        "name": ["Placebo"],
        "instanceType": ["StudyArm"]
      }
    }
  ]
}"#,
    )
    .expect("write suffix match dataset data");

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
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec![
            "parent_id".to_owned(),
            "parent_id.StudyArm".to_owned(),
            "rel_type.StudyArm".to_owned()
        ]
    );
}

#[test]
fn run_validation_joins_single_match_dataset_before_suffix_group_alias_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000799.json"),
        r#"{
  "Core": { "Id": "CORE-000799", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "parent_id", "Right": "id" }
      ]
    }
  ],
  "Operations": [
    {
      "group": ["id", "rel_type"],
      "group_aliases": ["id", "rel_type.StudyArm"],
      "id": "$parent_of_population",
      "name": "parent_id",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "parent_rel", "operator": "equal_to", "value": "populationIds", "value_is_literal": true },
      { "name": "rel_type.StudyArm", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id.StudyArm", "operator": "is_not_contained_by", "value": "$parent_of_population" }
    ]
  },
  "Outcome": {
    "Message": "Population and arm parents must match",
    "Output Variables": [
      "parent_entity",
      "parent_id",
      "parent_rel",
      "id",
      "name",
      "parent_id.StudyArm",
      "$parent_of_population"
    ]
  }
}"#,
    )
    .expect("write suffix group-alias rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyArm"],
        "parent_id": ["StudyDesign_2", "StudyArm_1"],
        "parent_rel": ["population", "populationIds"],
        "rel_type": ["definition", "reference"],
        "id": ["StudyDesignPopulation_1", "StudyDesignPopulation_1"],
        "name": ["POP1", "POP1"],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["arms"],
        "rel_type": ["definition"],
        "id": ["StudyArm_1"],
        "name": ["Placebo"],
        "instanceType": ["StudyArm"]
      }
    }
  ]
}"#,
    )
    .expect("write suffix group-alias data");

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
    assert!(outcome.results[0].errors[0]
        .variables
        .contains(&"parent_id.StudyArm".to_owned()));
    assert!(outcome.results[0].errors[0]
        .variables
        .contains(&"$parent_of_population".to_owned()));
}

#[test]
fn run_validation_passes_single_match_dataset_suffix_group_alias_operation_when_parent_matches() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000799.json"),
        r#"{
  "Core": { "Id": "CORE-000799", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["StudyDesignPopulation"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Match Datasets": [
    {
      "Name": "StudyArm",
      "Keys": [
        { "Left": "parent_id", "Right": "id" }
      ]
    }
  ],
  "Operations": [
    {
      "group": ["id", "rel_type"],
      "group_aliases": ["id", "rel_type.StudyArm"],
      "id": "$parent_of_population",
      "name": "parent_id",
      "operator": "distinct"
    }
  ],
  "Check": {
    "all": [
      { "name": "instanceType", "operator": "equal_to", "value": "StudyDesignPopulation" },
      { "name": "rel_type", "operator": "equal_to", "value": "reference" },
      { "name": "parent_rel", "operator": "equal_to", "value": "populationIds", "value_is_literal": true },
      { "name": "rel_type.StudyArm", "operator": "equal_to", "value": "definition" },
      { "name": "parent_id.StudyArm", "operator": "is_not_contained_by", "value": "$parent_of_population" }
    ]
  },
  "Outcome": {
    "Message": "Population and arm parents must match",
    "Output Variables": [
      "parent_entity",
      "parent_id",
      "parent_rel",
      "id",
      "name",
      "parent_id.StudyArm",
      "$parent_of_population"
    ]
  }
}"#,
    )
    .expect("write suffix group-alias rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "StudyDesignPopulation.csv",
      "domain": "StudyDesignPopulation",
      "records": {
        "parent_entity": ["StudyDesign", "StudyArm"],
        "parent_id": ["StudyDesign_1", "StudyArm_1"],
        "parent_rel": ["population", "populationIds"],
        "rel_type": ["definition", "reference"],
        "id": ["StudyDesignPopulation_1", "StudyDesignPopulation_1"],
        "name": ["POP1", "POP1"],
        "instanceType": ["StudyDesignPopulation", "StudyDesignPopulation"]
      }
    },
    {
      "filename": "StudyArm.csv",
      "domain": "StudyArm",
      "records": {
        "parent_id": ["StudyDesign_1"],
        "parent_rel": ["arms"],
        "rel_type": ["definition"],
        "id": ["StudyArm_1"],
        "name": ["Placebo"],
        "instanceType": ["StudyArm"]
      }
    }
  ]
}"#,
    )
    .expect("write suffix group-alias data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
        outcome.results[0].execution_status,
        ExecutionStatus::Passed,
        "{:?}",
        outcome.results[0]
    );
    assert_eq!(outcome.results[0].error_count, 0);
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
fn run_validation_reference_distinct_rdomain_variables_allows_missing_source_column() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-RDOMAIN-VARS.yml"),
        r#"Core:
  Id: CORE-RDOMAIN-VARS
  Status: Published
Scope:
  Domains:
    Include:
      - RELREC
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: RELREC
    id: $rdomain_variables
    name: IDVAR
    operator: distinct
    value_is_reference: true
Check:
  all:
    - name: RDOMAIN
      operator: exists
    - name: IDVAR
      operator: non_empty
    - name: IDVAR
      operator: is_not_contained_by
      value: $rdomain_variables
Outcome:
  Message: IDVAR must be present in RDOMAIN
  Output Variables:
    - IDVAR
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nrelrec,Related Records\nlb,Laboratory Test Results\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nrelrec,RDOMAIN,Related Domain,Char,8\nrelrec,USUBJID,Subject,Char,8\nlb,LBSEQ,Sequence,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(data_dir.join("relrec.csv"), "RDOMAIN,USUBJID\nLB,S001\n").expect("write relrec csv");
    fs::write(data_dir.join("lb.csv"), "LBSEQ\n1\n").expect("write lb csv");

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
fn run_validation_core_000039_assumes_missing_svpresp_is_planned() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000039.yml"),
        r#"Core:
  Id: CORE-000039
  Status: Published
Scope:
  Domains:
    Include:
      - SV
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TV
    id: $tv_visitnum
    name: VISITNUM
    operator: distinct
Check:
  all:
    - name: SVPRESP
      operator: equal_to
      value: Y
    - name: VISITNUM
      operator: is_not_contained_by
      value: $tv_visitnum
Outcome:
  Message: VISITNUM for planned visit is not in TV.
"#,
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
        "USUBJID": ["S1", "S2"],
        "VISITNUM": [5.01, 12.0],
        "VISITDY": [null, -28]
      }
    },
    {
      "filename": "tv.xpt",
      "domain": "TV",
      "records": {
        "VISITNUM": [5.0]
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
    assert_eq!(result.errors[0].variables, vec!["SVPRESP", "VISITNUM"]);
}

#[test]
fn run_validation_core_000660_passes_absent_to_reference_distinct_from_scoped_values() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000660.yml"),
        r#"Core:
  Id: CORE-000660
  Status: Published
Scope:
  Domains:
    Include:
      - ALL
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TO
    id: $to_sptobid
    name: SPTOBID
    operator: distinct
Check:
  all:
    - name: SPTOBID
      operator: is_not_contained_by
      value: $to_sptobid
Outcome:
  Message: SPTOBID must be represented in TO
  Output Variables:
    - SPTOBID
    - $to_sptobid
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\npd,Product Design\nem,Device Events\ntx,Trial Sets\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\npd,SPTOBID,Product,Char,16\nem,SPTOBID,Product,Char,16\ntx,TXPARMCD,Parameter,Char,16\ntx,TXVAL,Value,Char,16\n",
    )
    .expect("write variables csv");
    fs::write(data_dir.join("pd.csv"), "SPTOBID\nCIG01a\nVAPE-Z27\n").expect("write pd csv");
    fs::write(data_dir.join("em.csv"), "SPTOBID\nVAPE-Z01\n").expect("write em csv");
    fs::write(data_dir.join("tx.csv"), "TXPARMCD,TXVAL\nMETACTFL,Y\n").expect("write tx csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 3);
    assert!(outcome
        .results
        .iter()
        .all(|result| result.execution_status == ExecutionStatus::Passed));
}

#[test]
fn run_validation_core_000660_applies_to_non_to_datasets_when_to_is_loaded() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000660.yml"),
        r#"Core:
  Id: CORE-000660
  Status: Published
Scope:
  Domains:
    Include:
      - ALL
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TO
    id: $to_sptobid
    name: SPTOBID
    operator: distinct
Check:
  all:
    - name: SPTOBID
      operator: is_not_contained_by
      value: $to_sptobid
Outcome:
  Message: SPTOBID must be represented in TO
  Output Variables:
    - SPTOBID
    - $to_sptobid
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\npd,Product Design\nem,Device Events\nto,Tobacco Products\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\npd,SPTOBID,Product,Char,16\npd,PDSEQ,Sequence,Num,8\nem,SPTOBID,Product,Char,16\nem,EMSEQ,Sequence,Num,8\nto,SPTOBID,Product,Char,16\nto,TOSEQ,Sequence,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("pd.csv"),
        "SPTOBID,PDSEQ\nCIG01b,1\nCIG01a,2\n",
    )
    .expect("write pd csv");
    fs::write(data_dir.join("em.csv"), "SPTOBID,EMSEQ\nAPE-Z01,1\n").expect("write em csv");
    fs::write(
        data_dir.join("to.csv"),
        "SPTOBID,TOSEQ\nCIG01a,1\nVAPE-Z01,2\n",
    )
    .expect("write to csv");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0].dataset, "PD");
    assert_eq!(failed[0].errors[0].row, Some(1));
    assert_eq!(
        failed[0].errors[0].variables,
        vec!["SPTOBID", "$to_sptobid"]
    );
    assert_eq!(failed[1].dataset, "EM");
    assert_eq!(failed[1].errors[0].row, Some(1));
    assert_eq!(
        failed[1].errors[0].variables,
        vec!["SPTOBID", "$to_sptobid"]
    );
}

#[test]
fn run_validation_core_000840_uses_grouped_external_distinct_values() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000840.yml"),
        r#"Core:
  Id: CORE-000840
  Status: Published
Scope:
  Entities:
    Include:
      - Activity
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: ScheduleTimeline
    group:
      - parent_id
      - rel_type
    id: $timeline_ids_for_study_design
    name: id
    operator: distinct
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: Activity
      value_is_literal: true
    - name: rel_type
      operator: equal_to
      value: definition
      value_is_literal: true
    - name: timelineId
      operator: exists
    - name: timelineId
      operator: non_empty
    - name: timelineId
      operator: is_not_contained_by
      value: $timeline_ids_for_study_design
Outcome:
  Message: Activity references a timeline outside its study design
  Output Variables:
    - parent_entity
    - parent_id
    - parent_rel
    - id
    - name
    - timelineId
    - $timeline_ids_for_study_design
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nActivity,Activity\nScheduleTimeline,Schedule Timeline\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nActivity,parent_entity,Parent,Char,40\nActivity,parent_id,Parent ID,Char,40\nActivity,parent_rel,Parent Rel,Char,40\nActivity,rel_type,Rel Type,Char,40\nActivity,id,ID,Char,40\nActivity,name,Name,Char,40\nActivity,instanceType,Type,Char,40\nActivity,timelineId,Timeline,Char,40\nScheduleTimeline,parent_id,Parent ID,Char,40\nScheduleTimeline,rel_type,Rel Type,Char,40\nScheduleTimeline,id,ID,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("Activity.csv"),
        "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType,timelineId\nStudyDesign,StudyDesign_1,activities,definition,Activity_1,Informed consent,Activity,ScheduleTimeline_2\n",
    )
    .expect("write activity csv");
    fs::write(
        data_dir.join("ScheduleTimeline.csv"),
        "parent_id,rel_type,id\nStudyDesign_1,definition,ScheduleTimeline_1\nStudyDesign_2,definition,ScheduleTimeline_2\n",
    )
    .expect("write timeline csv");

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
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec![
            "parent_entity",
            "parent_id",
            "parent_rel",
            "id",
            "name",
            "timelineId",
            "$timeline_ids_for_study_design",
        ]
    );
}

#[test]
fn run_validation_core_000877_uses_group_aliases_for_external_distinct_values() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000877.yml"),
        r#"Core:
  Id: CORE-000877
  Status: Published
Scope:
  Entities:
    Include:
      - ScheduledActivityInstance
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: ScheduleTimeline
    group:
      - id
      - rel_type
    group_aliases:
      - parent_id
    id: $study_design_id_for_scheduled_instance
    name: parent_id
    operator: distinct
  - domain: ScheduleTimeline
    group:
      - id
      - rel_type
    group_aliases:
      - timelineId
    id: $study_design_id_for_subtimeline
    name: parent_id
    operator: distinct
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: ScheduledActivityInstance
      value_is_literal: true
    - name: rel_type
      operator: equal_to
      value: definition
      value_is_literal: true
    - name: timelineId
      operator: non_empty
    - name: $study_design_id_for_subtimeline
      operator: not_equal_to
      value: $study_design_id_for_scheduled_instance
Outcome:
  Message: Scheduled activity instance references a sub-timeline outside its study design
  Output Variables:
    - parent_entity
    - parent_id
    - parent_rel
    - id
    - name
    - timelineId
    - $study_design_id_for_scheduled_instance
    - $study_design_id_for_subtimeline
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nScheduledActivityInstance,Scheduled Activity Instance\nScheduleTimeline,Schedule Timeline\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nScheduledActivityInstance,parent_entity,Parent,Char,40\nScheduledActivityInstance,parent_id,Parent ID,Char,40\nScheduledActivityInstance,parent_rel,Parent Rel,Char,40\nScheduledActivityInstance,rel_type,Rel Type,Char,40\nScheduledActivityInstance,id,ID,Char,40\nScheduledActivityInstance,name,Name,Char,40\nScheduledActivityInstance,instanceType,Type,Char,40\nScheduledActivityInstance,timelineId,Timeline,Char,40\nScheduleTimeline,parent_id,Parent ID,Char,40\nScheduleTimeline,rel_type,Rel Type,Char,40\nScheduleTimeline,id,ID,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("ScheduledActivityInstance.csv"),
        "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType,timelineId\nScheduleTimeline,ScheduleTimeline_4,instances,definition,ScheduledActivityInstance_11,DOSE-1,ScheduledActivityInstance,ScheduleTimeline_14\n",
    )
    .expect("write scheduled activity instance csv");
    fs::write(
        data_dir.join("ScheduleTimeline.csv"),
        "parent_id,rel_type,id\nStudyDesign_1,definition,ScheduleTimeline_4\nStudyDesign_2,definition,ScheduleTimeline_14\n",
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
    let result = &outcome.results[0];
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec![
            "parent_entity",
            "parent_id",
            "parent_rel",
            "id",
            "name",
            "timelineId",
            "$study_design_id_for_scheduled_instance",
            "$study_design_id_for_subtimeline",
        ]
    );
}

#[test]
fn run_validation_core_000868_filters_grouped_external_distinct_source_rows() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000868.yml"),
        r#"Core:
  Id: CORE-000868
  Status: Published
Scope:
  Entities:
    Include:
      - ScheduleTimeline
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: Timing
    filter:
      rel_type: definition
      type.code: C201358
    group:
      - parent_id
    group_aliases:
      - id
    id: $fixed_ref_sched_ins
    name: relativeFromScheduledInstanceId
    operator: distinct
  - domain: ScheduledActivityInstance
    filter:
      rel_type: definition
    group:
      - parent_id
    group_aliases:
      - id
    id: $instances
    name: id
    operator: distinct
Check:
  all:
    - name: instanceType
      operator: equal_to
      value: ScheduleTimeline
      value_is_literal: true
    - name: rel_type
      operator: equal_to
      value: definition
      value_is_literal: true
    - any:
        - name: $fixed_ref_sched_ins
          operator: empty
        - name: $instances
          operator: empty
        - name: $fixed_ref_sched_ins
          operator: shares_no_elements_with
          value: $instances
Outcome:
  Message: Schedule timeline does not contain a fixed reference anchor
  Output Variables:
    - parent_entity
    - parent_id
    - parent_rel
    - id
    - name
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\nScheduleTimeline,Schedule Timeline\nScheduledActivityInstance,Scheduled Activity Instance\nTiming,Timing\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\nScheduleTimeline,parent_entity,Parent,Char,40\nScheduleTimeline,parent_id,Parent ID,Char,40\nScheduleTimeline,parent_rel,Parent Rel,Char,40\nScheduleTimeline,rel_type,Rel Type,Char,40\nScheduleTimeline,id,ID,Char,40\nScheduleTimeline,name,Name,Char,40\nScheduleTimeline,instanceType,Type,Char,40\nScheduledActivityInstance,parent_id,Parent ID,Char,40\nScheduledActivityInstance,rel_type,Rel Type,Char,40\nScheduledActivityInstance,id,ID,Char,40\nTiming,parent_id,Parent ID,Char,40\nTiming,rel_type,Rel Type,Char,40\nTiming,type.code,Type Code,Char,40\nTiming,relativeFromScheduledInstanceId,From,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        data_dir.join("ScheduleTimeline.csv"),
        "parent_entity,parent_id,parent_rel,rel_type,id,name,instanceType\nStudyDesign,StudyDesign_1,scheduleTimelines,definition,ScheduleTimeline_1,Adverse Event Timeline,ScheduleTimeline\n",
    )
    .expect("write schedule timeline csv");
    fs::write(
        data_dir.join("ScheduledActivityInstance.csv"),
        "parent_id,rel_type,id\nScheduleTimeline_1,definition,ScheduledActivityInstance_1\n",
    )
    .expect("write scheduled activity instance csv");
    fs::write(
        data_dir.join("Timing.csv"),
        "parent_id,rel_type,type.code,relativeFromScheduledInstanceId\nScheduleTimeline_1,definition,C201356,ScheduledActivityInstance_1\n",
    )
    .expect("write timing csv");

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
    assert_eq!(result.errors[0].row, Some(1));
    assert_eq!(
        result.errors[0].variables,
        vec!["parent_entity", "parent_id", "parent_rel", "id", "name"]
    );
}

#[test]
fn run_validation_core_000676_passes_absent_to_reference_distinct_when_sptobid_parameter_absent() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000676.yml"),
        r#"Core:
  Id: CORE-000676
  Status: Published
Scope:
  Domains:
    Include:
      - TX
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TO
    id: $to_sptobid
    name: SPTOBID
    operator: distinct
Check:
  all:
    - name: TXPARMCD
      operator: equal_to
      value: SPTOBID
      value_is_literal: true
    - name: TXVAL
      operator: is_not_contained_by
      value: $to_sptobid
Outcome:
  Message: TXVAL SPTOBID must be represented in TO
  Output Variables:
    - TXVAL
    - $to_sptobid
"#,
    )
    .expect("write rule");
    fs::write(
        data_dir.join("_datasets.csv"),
        "Filename,Label\ntx,Trial Sets\n",
    )
    .expect("write datasets csv");
    fs::write(
        data_dir.join("_variables.csv"),
        "dataset,variable,label,type,length\ntx,TXPARMCD,Parameter,Char,16\ntx,TXVAL,Value,Char,16\n",
    )
    .expect("write variables csv");
    fs::write(data_dir.join("tx.csv"), "TXPARMCD,TXVAL\nMETACTFL,Y\n").expect("write tx csv");

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
