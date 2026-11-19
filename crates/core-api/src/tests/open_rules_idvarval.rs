use std::fs;

use core_engine::{ExecutionStatus, RuleValidationResult};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

const PUBLISHED_OUTPUT_VARIABLES: &[&str] = &["RDOMAIN", "IDVAR", "IDVARVAL"];

#[test]
fn core_000206_negative_01_rejects_lb_references_for_the_wrong_subject() {
    let result = run_core_000206(
        &[
            ("co", "S,CO,LB,S001,1,LBGRPID,20\n"),
            (
                "relrec",
                "S,LB,S001,LBSEQ,320,ONE,1\nS,AE,S001,AESEQ,2,ONE,2\n",
            ),
            ("supplb", "S,LB,S001,LBSEQ,320,LBCLSIG,Y\n"),
            ("lb", "S,LB,S001,321,21\nS,LB,S002,320,20\n"),
            ("ae", "S,AE,S001,1\n"),
        ],
        PUBLISHED_OUTPUT_VARIABLES,
    );
    assert_core_000206_issues(
        &result,
        &[("CO", 1), ("RELREC", 1), ("RELREC", 2), ("SUPPLB", 1)],
        PUBLISHED_OUTPUT_VARIABLES,
    );
}

#[test]
fn core_000206_negative_02_reports_globally_absent_values_and_unknown_idvar_columns() {
    let result = run_core_000206(
        &[
            (
                "co",
                "S,CO,LB,S001,1,LBSEQ,320\nS,CO,LB,S001,1,NONSENSE,299\n",
            ),
            (
                "relrec",
                "S,LB,S001,LBSEQ,320,ONE,1\nS,LB,S001,NONSENSE,299,ONE,1\n",
            ),
            ("supplb", "S,LB,S001,LBSEQ,320,LBCLSIG,Y\n"),
            ("lb", "S,LB,S001,299,\n"),
            ("ae", ""),
        ],
        PUBLISHED_OUTPUT_VARIABLES,
    );
    assert_core_000206_issues(
        &result,
        &[
            ("CO", 1),
            ("CO", 2),
            ("RELREC", 1),
            ("RELREC", 2),
            ("SUPPLB", 1),
        ],
        PUBLISHED_OUTPUT_VARIABLES,
    );
}

#[test]
fn core_000206_negative_03_reports_only_invalid_sibling_rows_once() {
    let result = run_core_000206(
        &[
            ("co", "S,CO,LB,S001,1,LBGRPID,22\nS,CO,AE,S001,2,AESEQ,5\n"),
            (
                "relrec",
                "S,LB,S001,LBSEQ,320,ONE,1\nS,LB,S001,LBSEQ,323,ONE,1\nS,AE,S001,AESEQ,1,ONE,2\n",
            ),
            (
                "supplb",
                "S,LB,S001,LBSEQ,321,LBCLSIG,Y\nS,LB,S001,LBSEQ,999,LBCLSIG,Y\n",
            ),
            (
                "lb",
                "S,LB,S001,320,20\nS,LB,S001,321,21\nS,LB,S001,322,22\nS,LB,S002,323,23\n",
            ),
            ("ae", "S,AE,S001,1\nS,AE,S001,2\n"),
        ],
        PUBLISHED_OUTPUT_VARIABLES,
    );
    // The official compound-keys control isolates the bad rows from subject-only fanout.
    assert_core_000206_issues(
        &result,
        &[("CO", 2), ("RELREC", 2), ("SUPPLB", 2)],
        PUBLISHED_OUTPUT_VARIABLES,
    );
}

#[test]
fn run_validation_passes_core_000206_when_idvarval_values_exist_in_rdomain_records() {
    let result = run_core_000206(
        &[
            (
                "co",
                "S,CO,LB,S001,1,LBGRPID,20\nS,CO,LB,S002,2,LBSEQ,321\n",
            ),
            (
                "relrec",
                "S,LB,S001,LBSEQ,320,ONE,1\nS,AE,S001,AESEQ,1,ONE,2\n",
            ),
            ("supplb", "S,LB,S001,LBSEQ,320,LBCLSIG,Y\n"),
            ("lb", "S,LB,S001,320,20\nS,LB,S002,321,30\n"),
            ("ae", "S,AE,S001,1\n"),
        ],
        PUBLISHED_OUTPUT_VARIABLES,
    );
    assert_eq!(result.execution_status, ExecutionStatus::Passed);
    assert_eq!(result.error_count, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn core_000206_legacy_rule_reports_both_missing_supp_references_and_projects_usubjid() {
    let legacy_output_variables = &["RDOMAIN", "USUBJID", "IDVAR", "IDVARVAL"];
    let result = run_core_000206(
        &[
            ("co", ""),
            ("relrec", ""),
            (
                "supplb",
                "S,LB,S001,LBSEQ,320,LBCLSIG,Y\nS,LB,S001,LBGRPID,20,LBCLSIG,Y\n",
            ),
            ("lb", "S,LB,S001,299,21\nS,LB,S002,319,20\n"),
            ("ae", ""),
        ],
        legacy_output_variables,
    );
    assert_core_000206_issues(
        &result,
        &[("SUPPLB", 1), ("SUPPLB", 2)],
        legacy_output_variables,
    );
}

fn assert_core_000206_issues(
    result: &RuleValidationResult,
    expected_rows: &[(&str, usize)],
    output_variables: &[&str],
) {
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    let mut rows = result
        .errors
        .iter()
        .map(|issue| (issue.dataset.as_str(), issue.row.expect("issue row")))
        .collect::<Vec<_>>();
    rows.sort_unstable();
    assert_eq!(rows.as_slice(), expected_rows);
    assert_eq!(result.error_count, expected_rows.len());
    for issue in &result.errors {
        assert_eq!(issue.rule_id, "CORE-000206");
        assert_eq!(issue.usubjid.as_deref(), Some("S001"));
        let variables = issue
            .variables
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(variables.as_slice(), output_variables);
    }
}

fn run_core_000206(datasets: &[(&str, &str)], output_variables: &[&str]) -> RuleValidationResult {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    write_core_000206_rule(&rules_dir, output_variables);
    write_core_000206_open_rules_metadata(&data_dir);
    for &(dataset, rows) in datasets {
        let header = match dataset {
            "co" => "STUDYID,DOMAIN,RDOMAIN,USUBJID,COSEQ,IDVAR,IDVARVAL",
            "relrec" => "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,RELTYPE,RELID",
            "supplb" => "STUDYID,RDOMAIN,USUBJID,IDVAR,IDVARVAL,QNAM,QVAL",
            "lb" => "STUDYID,DOMAIN,USUBJID,LBSEQ,LBGRPID",
            "ae" => "STUDYID,DOMAIN,USUBJID,AESEQ",
            _ => panic!("unexpected fixture dataset: {dataset}"),
        };
        fs::write(
            data_dir.join(format!("{dataset}.csv")),
            format!("{header}\n{rows}"),
        )
        .unwrap_or_else(|error| panic!("write {dataset} csv: {error}"));
    }

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![data_dir],
        dataset_loader: DatasetLoader::OpenRulesDataDir,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    let result = outcome.results.into_iter().next().expect("rule result");
    assert_eq!(result.rule_id, "CORE-000206");
    result
}

fn write_core_000206_rule(rules_dir: &std::path::Path, output_variables: &[&str]) {
    fs::write(
        rules_dir.join("CORE-000206.yml"),
        format!(
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
    Name: SUPP--
  - Child: true
    Keys:
      - USUBJID
    Name: CO
  - Child: true
    Keys:
      - USUBJID
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
  Output Variables: [{}]
"#,
            output_variables.join(", ")
        ),
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
