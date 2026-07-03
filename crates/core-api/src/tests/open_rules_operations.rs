use std::fs;

use core_engine::{ExecutionStatus, SkippedReason};
use core_rule_model::load_rules_from_paths;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::operation_fields::operation_name;
use crate::{run_validation, DatasetLoader, ValidateRequest};

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
