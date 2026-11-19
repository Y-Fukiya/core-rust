use std::fs;

use core_engine::{ExecutionStatus, RuleValidationResult, SkippedReason};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_core_000609_uses_one_tx_set_across_dm_relref_and_tx() {
    let datasets: &[(&str, &[&str])] = &[
        ("DM", &["A1", "a1", "NON_TX", "A2", ""]),
        ("RELREF", &["NON_TX", "A2", "", "a2", "A1"]),
        ("TX", &["A1", "", "A2", "A1"]),
    ];
    for compat in [false, true] {
        let results = run_core_000609_fixture(datasets, compat);
        assert_core_000609_findings(
            &results,
            &[("DM", &[2, 3, 5]), ("RELREF", &[1, 3, 4]), ("TX", &[2])],
            compat,
        );
    }
}

#[test]
fn run_validation_core_000609_skips_when_tx_is_absent() {
    let datasets: &[(&str, &[&str])] = &[("DM", &["A1"]), ("RELREF", &[""])];
    for compat in [false, true] {
        let results = run_core_000609_fixture(datasets, compat);
        assert_eq!(results.len(), 1, "compat={compat}: {results:?}");
        let result = &results[0];
        assert_eq!(result.rule_id, "CORE-000609");
        assert_eq!(
            result.execution_status,
            ExecutionStatus::Skipped,
            "compat={compat}"
        );
        assert_eq!(result.error_count, 0);
        assert!(result.errors.is_empty());
    }
}

#[test]
fn run_validation_core_000609_reports_blank_when_tx_is_the_only_dataset() {
    for compat in [false, true] {
        let results = run_core_000609_fixture(&[("TX", &["A1", "", "A2"])], compat);
        assert_core_000609_findings(&results, &[("TX", &[2])], compat);
    }
}

#[test]
fn run_validation_core_000609_rejects_all_rows_when_tx_has_no_non_empty_setcd() {
    let datasets: &[(&str, &[&str])] = &[("DM", &["A1", ""]), ("TX", &["", ""])];
    for compat in [false, true] {
        let results = run_core_000609_fixture(datasets, compat);
        assert_core_000609_findings(&results, &[("DM", &[1, 2]), ("TX", &[1, 2])], compat);
    }
}

#[test]
fn run_validation_core_000609_preserves_csv_setcd_and_ignores_lookalike_columns() {
    let results = run_core_000609_generic_csv(&[CORE_000609_RULE], TX_WITH_SETCD_LOOKALIKE);
    let dm = results
        .iter()
        .find(|result| result.dataset == "DM")
        .expect("DM result");
    assert_eq!(dm.execution_status, ExecutionStatus::Failed);
    assert_eq!(
        dm.errors.iter().map(|issue| issue.row).collect::<Vec<_>>(),
        [Some(2), Some(3)]
    );
    assert_eq!(
        dm.execution_provenance.as_deref(),
        Some("rule_specific_engine_semantics")
    );
    let tx = results
        .iter()
        .find(|result| result.dataset == "TX")
        .expect("TX result");
    assert_eq!(tx.execution_status, ExecutionStatus::Passed);
}

#[test]
fn run_validation_core_000609_does_not_claim_the_adapter_for_a_different_check() {
    let rule = CORE_000609_RULE.replace(
        "operator: is_not_contained_by\n      value: $tx_setcd",
        "operator: empty",
    );
    let results = run_core_000609_generic_csv(&[&rule], TX_WITH_SETCD_LOOKALIKE);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(
        results[0].execution_provenance.as_deref(),
        Some("generic_engine")
    );
}

#[test]
fn run_validation_core_000609_keeps_other_rules_on_the_inferred_csv_values() {
    let grouped_distinct = r#"Core:
  Id: CORE-000891
  Status: Published
Scope:
  Domains:
    Include: [TX]
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TX
    group: [SETCD]
    id: $txparmcd
    name: TXPARMCD
    operator: distinct
Check:
  name: $txparmcd
  operator: does_not_contain
  value: ARMCD
Outcome:
  Message: Each group must contain ARMCD
  Output Variables: [SETCD, $txparmcd]
"#;
    let tx = "SETCD,TXPARMCD\n1E2,ARMCD\n1e2,SPECIES\n";
    let alone = run_core_000609_generic_csv(&[grouped_distinct], tx);
    let mixed = run_core_000609_generic_csv(&[CORE_000609_RULE, grouped_distinct], tx);
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].execution_status, ExecutionStatus::Passed);
    assert_eq!(
        mixed.iter().find(|result| result.rule_id == "CORE-000891"),
        Some(&alone[0])
    );
    assert!(mixed.iter().any(|result| {
        result.rule_id == "CORE-000609" && result.execution_status == ExecutionStatus::Failed
    }));
}

const TX_WITH_SETCD_LOOKALIKE: &str = "$SETCD,SETCD,TXPARMCD\nA1,1E2,ARMCD\nA1,1e2,SPECIES\n";

fn run_core_000609_generic_csv(rules: &[&str], tx: &str) -> Vec<RuleValidationResult> {
    let dir = tempdir().expect("tempdir");
    let rule_paths = rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let path = dir.path().join(format!("rule_{index}.yml"));
            fs::write(&path, rule).expect("write rule");
            path
        })
        .collect();
    let tx_path = dir.path().join("tx.csv");
    let dm_path = dir.path().join("dm.csv");
    fs::write(&tx_path, tx).expect("write TX");
    fs::write(&dm_path, "$SETCD,SETCD\nA1,1E2\n1E2,A1\n1E2,1e+2\n").expect("write DM");
    run_validation(ValidateRequest {
        rule_paths,
        dataset_paths: vec![dm_path, tx_path],
        ..Default::default()
    })
    .expect("run CORE-000609 with generic CSV")
    .results
}

const CORE_000609_RULE: &str = r#"Core:
  Id: CORE-000609
  Status: Published
Scope:
  Classes:
    Include: [ALL]
  Domains:
    Include: [ALL]
  Use Case: NONCLIN
Sensitivity: Record
Rule Type: Record Data
Operations:
  - domain: TX
    id: $tx_setcd
    name: SETCD
    operator: distinct
Check:
  all:
    - name: SETCD
      operator: is_not_contained_by
      value: $tx_setcd
Outcome:
  Message: SETCD must match a value in TX
  Output Variables: [SETCD, $tx_setcd]
"#;

fn run_core_000609_fixture(
    datasets: &[(&str, &[&str])],
    compat: bool,
) -> Vec<RuleValidationResult> {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("CORE-000609.yml");
    let data_dir = dir.path().join("data");
    fs::create_dir(&data_dir).expect("data dir");
    fs::write(&rule_path, CORE_000609_RULE).expect("write CORE-000609 rule");
    let mut manifest = String::from("Filename,Label\n");
    let mut variables = String::from("dataset,variable,label,type,length\n");
    for &(domain, setcds) in datasets {
        let filename = domain.to_ascii_lowercase();
        manifest.push_str(&format!("{filename},{domain}\n"));
        variables.push_str(&format!(
            "{filename},DOMAIN,Domain,Char,8\n{filename},SETCD,Set Code,Char,16\n"
        ));
        let mut csv = String::from("DOMAIN,SETCD\n");
        for setcd in setcds {
            csv.push_str(&format!("{domain},{setcd}\n"));
        }
        fs::write(data_dir.join(format!("{filename}.csv")), csv).expect("write dataset CSV");
    }
    fs::write(data_dir.join("_datasets.csv"), manifest).expect("write dataset manifest");
    fs::write(data_dir.join("_variables.csv"), variables).expect("write variable schema");

    run_validation(ValidateRequest {
        rule_paths: vec![rule_path],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        open_rules_oracle_compat: compat,
        ..Default::default()
    })
    .expect("run CORE-000609")
    .results
}

fn assert_core_000609_findings(
    results: &[RuleValidationResult],
    expected: &[(&str, &[usize])],
    compat: bool,
) {
    assert_eq!(
        results.len(),
        expected.len(),
        "compat={compat}: {results:?}"
    );
    for &(dataset, rows) in expected {
        let result = results
            .iter()
            .find(|result| result.dataset == dataset)
            .unwrap_or_else(|| panic!("missing {dataset} result, compat={compat}: {results:?}"));
        assert_eq!(result.rule_id, "CORE-000609");
        assert_eq!(
            result.execution_status,
            ExecutionStatus::Failed,
            "{dataset}, compat={compat}"
        );
        assert_eq!(result.error_count, rows.len(), "{dataset}, compat={compat}");
        let mut actual_rows = Vec::new();
        for issue in &result.errors {
            assert_eq!(issue.rule_id, "CORE-000609");
            assert_eq!(issue.dataset, dataset);
            assert_eq!(issue.variables, ["SETCD", "$tx_setcd"]);
            actual_rows.push(issue.row.expect("record row"));
        }
        actual_rows.sort_unstable();
        assert_eq!(actual_rows, rows, "{dataset}, compat={compat}");
    }
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
