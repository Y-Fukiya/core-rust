use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn study_domain_presence_checks_only_loaded_domains() {
    let dir = tempdir().expect("fixture");
    let rule = dir.path().join("rule.yml");
    let data = dir.path().join("datasets.json");
    fs::write(
        &rule,
        r#"Core: {Id: CORE-999581, Status: Published}
Rule Type: Domain Presence Check
Sensitivity: Study
Check: {name: DM, operator: not_exists}
Outcome: {Message: Missing DM, Output Variables: [DM]}
"#,
    )
    .expect("write rule");
    let ae = serde_json::json!({"filename": "ae.csv", "domain": "AE",
        "records": {"DM": ["not a DM dataset", "another row"], "AESEQ": [1, 2]}});
    let dm = serde_json::json!({"filename": "dm.csv", "domain": "DM",
        "records": {"USUBJID": ["01"]}});
    for compat in [false, true] {
        for has_dm in [false, true] {
            let datasets = if has_dm {
                vec![ae.clone(), dm.clone()]
            } else {
                vec![ae.clone()]
            };
            fs::write(&data, serde_json::json!({"datasets": datasets}).to_string())
                .expect("write data");
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![rule.clone()],
                dataset_paths: vec![data.clone()],
                open_rules_oracle_compat: compat,
                ..Default::default()
            })
            .expect("validate");
            let [result] = outcome.results.as_slice() else {
                panic!("expected exactly one study-level result");
            };
            assert_eq!(result.dataset, "STUDY");
            assert_eq!(result.domain, None);
            if has_dm {
                assert_eq!(result.execution_status, ExecutionStatus::Passed);
                assert!(result.errors.is_empty());
            } else {
                assert_eq!(result.execution_status, ExecutionStatus::Failed);
                let [issue] = result.errors.as_slice() else {
                    panic!("expected exactly one missing-domain finding");
                };
                assert_eq!(issue.variables, ["DM"]);
                assert_eq!(
                    (issue.row, &issue.usubjid, &issue.seq),
                    (None, &None, &None)
                );
            }
        }
    }
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
      "records": {
        "DOMAIN": ["TT", "TT"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "TTSEQ": [1, 2]
      }
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

    for compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path.clone()],
            output_dir: Some(dir.path().join(format!("reports-{compat}"))),
            open_rules_oracle_compat: compat,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.dataset, "STUDY");
        assert_eq!(result.domain, None);
        assert_eq!(result.errors.len(), 1);
        let issue = &result.errors[0];
        assert_eq!(issue.dataset, "STUDY");
        assert_eq!(issue.domain, None);
        assert_eq!(issue.variables, vec!["TT"]);
        assert_eq!(issue.row, None);
        assert_eq!(issue.usubjid, None);
        assert_eq!(issue.seq, None);

        let reports = outcome.reports.expect("reports");
        let report: serde_json::Value = serde_json::from_slice(
            &fs::read(reports.json.expect("json report")).expect("read json report"),
        )
        .expect("parse json report");
        assert_eq!(report["summary"]["error_count"], 1);
        assert_eq!(
            report["results"],
            serde_json::to_value(&outcome.results).unwrap()
        );

        let mut csv =
            csv::Reader::from_path(reports.csv.expect("csv report")).expect("read csv report");
        let rows = csv
            .deserialize::<std::collections::BTreeMap<String, String>>()
            .collect::<Result<Vec<_>, _>>()
            .expect("parse csv report");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["rule_id"], "CORE-DOMAIN-PRESENCE");
        assert_eq!(rows[0]["dataset"], "STUDY");
        assert_eq!(rows[0]["domain"], "");
        assert_eq!(rows[0]["variables"], "TT");
        assert_eq!(rows[0]["error_count"], "1");
        for locator in ["row", "usubjid", "seq"] {
            assert_eq!(rows[0][locator], "");
        }
    }
}

#[test]
fn run_validation_domain_presence_absent_target_keeps_no_findings() {
    let dir = tempdir().expect("tempdir");
    let rule_path = dir.path().join("rule.json");
    let dataset_path = dir.path().join("ae.csv");
    fs::write(&dataset_path, "DOMAIN,USUBJID,AESEQ\nAE,SUBJ1,1\n").expect("write dataset");
    fs::write(
        &rule_path,
        r#"{
  "Core": { "Id": "CORE-DOMAIN-PRESENCE", "Status": "Published" },
  "Sensitivity": "Dataset",
  "Rule Type": "Domain Presence Check",
  "Check": { "name": "TT", "operator": "exists" },
  "Outcome": { "Message": "TT dataset is present" }
}"#,
    )
    .expect("write rule");

    for compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rule_path.clone()],
            dataset_paths: vec![dataset_path.clone()],
            open_rules_oracle_compat: compat,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Passed);
        assert_eq!(result.error_count, 0);
        assert!(result.errors.is_empty());
        assert_eq!(result.dataset, "STUDY");
        assert_eq!(result.domain, None);
    }
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

    for compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.clone()],
            dataset_paths: vec![dataset_path.clone()],
            open_rules_oracle_compat: compat,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.dataset, "STUDY");
        assert_eq!(result.domain, None);
        assert_eq!(result.errors[0].dataset, "STUDY");
        assert_eq!(result.errors[0].domain, None);
        assert_eq!(result.errors[0].row, None);
        assert_eq!(result.errors[0].usubjid, None);
        assert_eq!(result.errors[0].seq, None);
        assert_eq!(
            result.errors[0].variables,
            vec!["$poolid_exists", "POOLDEF"]
        );
    }
}

fn write_pooldef_rules(dir: &std::path::Path) {
    fs::write(
        dir.join("CORE-000677.yml"),
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
  Message: POOLID variable exists in the dataset but POOLDEF dataset does not exist.
"#,
    )
    .expect("write 677 rule");
    fs::write(
        dir.join("CORE-000896.yml"),
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
"#,
    )
    .expect("write 896 rule");
}

#[test]
fn run_validation_pooldef_rules_when_pooldef_is_absent() {
    let dir = tempdir().expect("tempdir");
    write_pooldef_rules(dir.path());
    let dataset = dir.path().join("vs.csv");

    for (csv, expected_count) in [
        (
            "DOMAIN,USUBJID,VSSEQ,POOLID\nVS,SUBJ1,7,POOL99\nVS,SUBJ2,8,POOL98\n",
            1,
        ),
        ("DOMAIN,USUBJID,VSSEQ,POOLID\nVS,SUBJ1,7,\n", 1),
        ("DOMAIN,USUBJID,VSSEQ\nVS,SUBJ1,7\n", 0),
    ] {
        fs::write(&dataset, csv).expect("write VS");
        for compat in [false, true] {
            let outcome = run_validation(ValidateRequest {
                rule_paths: vec![dir.path().to_path_buf()],
                dataset_paths: vec![dataset.clone()],
                open_rules_oracle_compat: compat,
                ..Default::default()
            })
            .expect("run validation");

            assert_eq!(outcome.results.len(), 2);
            let presence = outcome
                .results
                .iter()
                .find(|r| r.rule_id == "CORE-000677")
                .unwrap();
            assert_eq!(
                presence.error_count, expected_count,
                "compat={compat}, VS={csv}"
            );
            assert_eq!(presence.errors.len(), expected_count);
            assert_eq!(presence.dataset, "STUDY");
            assert_eq!(presence.domain, None);
            assert_eq!(
                presence.execution_provenance.as_deref(),
                Some("generic_engine")
            );
            if expected_count == 0 {
                assert_eq!(presence.execution_status, ExecutionStatus::Passed);
            } else {
                assert_eq!(presence.execution_status, ExecutionStatus::Failed);
                let issue = &presence.errors[0];
                assert_eq!(issue.rule_id, "CORE-000677");
                assert_eq!(issue.dataset, "STUDY");
                assert_eq!(issue.domain, None);
                assert_eq!(issue.variables, vec!["$poolid_exists", "POOLDEF"]);
                assert_eq!(
                    (issue.row, &issue.usubjid, &issue.seq),
                    (None, &None, &None)
                );
            }

            if !compat {
                let membership = outcome
                    .results
                    .iter()
                    .find(|r| r.rule_id == "CORE-000896")
                    .unwrap();
                assert_eq!(membership.execution_status, ExecutionStatus::Passed);
                assert_eq!(membership.error_count, 0);
                assert!(membership.errors.is_empty());
            }
        }
    }
}

#[test]
fn run_validation_pooldef_presence_with_empty_dm_anchor() {
    assert_pooldef_presence_with_empty_anchor(true, "VS,SUBJ1,7,POOL99\n");
}

#[test]
fn run_validation_pooldef_presence_with_empty_vs_anchor() {
    assert_pooldef_presence_with_empty_anchor(false, "");
}

fn assert_pooldef_presence_with_empty_anchor(include_dm: bool, vs_rows: &str) {
    let dir = tempdir().expect("tempdir");
    write_pooldef_rules(dir.path());
    let data_dir = dir.path().join("data");
    fs::create_dir(&data_dir).expect("create data dir");
    let (datasets, dm_variables) = if include_dm {
        fs::write(data_dir.join("dm.csv"), "DOMAIN\n").expect("write header-only DM");
        ("dm,Demographics\nvs,Vital Signs\n", "dm,DOMAIN,Char\n")
    } else {
        ("vs,Vital Signs\n", "")
    };
    fs::write(
        data_dir.join("_datasets.csv"),
        format!("Filename,Label\n{datasets}"),
    )
    .expect("write dataset manifest");
    fs::write(
        data_dir.join("_variables.csv"),
        format!(
            "dataset,variable,type\n{dm_variables}vs,DOMAIN,Char\n\
             vs,USUBJID,Char\nvs,VSSEQ,Num\nvs,POOLID,Char\n"
        ),
    )
    .expect("write variable metadata");
    fs::write(
        data_dir.join("vs.csv"),
        format!("DOMAIN,USUBJID,VSSEQ,POOLID\n{vs_rows}"),
    )
    .expect("write VS");

    for compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![dir.path().join("CORE-000677.yml")],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            open_rules_oracle_compat: compat,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.error_count, 1, "compat={compat}");
        assert_eq!(result.execution_status, ExecutionStatus::Failed);
        assert_eq!(result.errors.len(), 1);
        let issue = &result.errors[0];
        assert_eq!(issue.rule_id, "CORE-000677");
        assert_eq!(issue.dataset, "STUDY");
        assert_eq!(issue.domain, None);
        assert_eq!(issue.variables, vec!["$poolid_exists", "POOLDEF"]);
        assert_eq!(
            (issue.row, &issue.usubjid, &issue.seq),
            (None, &None, &None)
        );
    }
}

#[test]
fn run_validation_pooldef_rules_distinguish_presence_from_membership() {
    let dir = tempdir().expect("tempdir");
    write_pooldef_rules(dir.path());
    let data_dir = dir.path().join("data");
    fs::create_dir(&data_dir).expect("create data dir");
    for (name, contents) in [
        (
            "_datasets.csv",
            "Filename,Label\nvs,Vital Signs\npooldef,Pool Definition\n",
        ),
        (
            "_variables.csv",
            "dataset,variable,label,type,length\n\
vs,DOMAIN,Domain Abbreviation,Char,2\n\
vs,USUBJID,Unique Subject Identifier,Char,8\n\
vs,POOLID,Pool Identifier,Char,8\n\
vs,VSSEQ,Sequence Number,Char,8\n\
pooldef,POOLID,Pool Identifier,Char,8\n",
        ),
        ("pooldef.csv", "POOLID\nPOOL1\n"),
        (
            "vs.csv",
            "DOMAIN,USUBJID,POOLID,VSSEQ\n\
VS,SUBJ1,POOL1,1\n\
VS,SUBJ2,POOL99,8\n\
VS,SUBJ3,,9\n",
        ),
    ] {
        fs::write(data_dir.join(name), contents).expect("write Open Rules data");
    }

    for compat in [false, true] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![dir.path().to_path_buf()],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            open_rules_oracle_compat: compat,
            ..Default::default()
        })
        .expect("run validation");

        assert_eq!(outcome.results.len(), 2);
        let membership = outcome
            .results
            .iter()
            .find(|r| r.rule_id == "CORE-000896")
            .unwrap();
        if compat {
            assert_eq!(membership.execution_status, ExecutionStatus::Skipped);
        } else {
            assert_eq!(membership.execution_status, ExecutionStatus::Failed);
            assert_eq!(membership.error_count, 1);
            assert_eq!(membership.errors.len(), 1);
            let issue = &membership.errors[0];
            assert_eq!(issue.rule_id, "CORE-000896");
            assert_eq!(issue.dataset, "VS");
            assert_eq!(issue.domain.as_deref(), Some("VS"));
            assert_eq!(issue.row, Some(2));
            assert_eq!(issue.usubjid.as_deref(), Some("SUBJ2"));
            assert_eq!(issue.seq.as_deref(), Some("8"));
            assert_eq!(issue.variables, vec!["$pooldef_poolid", "POOLID"]);
        }

        let presence = outcome
            .results
            .iter()
            .find(|r| r.rule_id == "CORE-000677")
            .unwrap();
        assert_eq!(
            presence.execution_status,
            ExecutionStatus::Passed,
            "compat={compat}"
        );
        assert_eq!(presence.dataset, "STUDY");
        assert_eq!(presence.domain, None);
        assert_eq!(presence.error_count, 0);
        assert!(presence.errors.is_empty());
    }
}

#[test]
fn run_validation_reports_core_000013_at_dataset_scope() {
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
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(result.errors[0].variables, vec!["AESTAT"]);
    assert_eq!(result.errors[0].usubjid, None);
    assert_eq!(result.errors[0].seq, None);
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
