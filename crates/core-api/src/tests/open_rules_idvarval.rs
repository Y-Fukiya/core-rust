use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

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
