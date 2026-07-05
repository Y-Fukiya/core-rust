use std::fs;

use core_data::load_dataset_package_json;
use core_rule_model::{
    ActionSpec, Condition, ConditionGroup, ExecutableRule, Operator, OperatorOptions, RuleType,
    Sensitivity, ValueExpr,
};
use serde_json::{json, Value};
use tempfile::tempdir;

use crate::LoadedDataset;

pub(super) fn test_dataset() -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "ae.xpt",
  "domain": "AE",
    "records": {
    "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3", "SUBJ4"],
    "AESEQ": [1, 2, 3, null],
    "AESEQ_COPY": [1, 20, 3, null],
    "DOMAIN": ["AE", "CM", "", null],
    "TERM": ["Headache", "nausea", "", null],
    "STARTDTC": ["2024-01-02", "2024-01-03T12:30:00", "2024-01", "2024-13-01"],
    "DUR": ["P1D", "PT2H", "P1Y2M", "P-1D"],
    "FLAG": [true, false, null, true]
  }
}
  ]
}"#,
    )
    .expect("write dataset package");

    load_dataset_package_json(&path)
        .expect("load dataset package")
        .into_iter()
        .next()
        .expect("dataset")
}

pub(super) fn sort_dataset() -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "ae.xpt",
  "domain": "AE",
  "records": {
    "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2", "SUBJ2"],
    "AESEQ": [1, 3, 2, 1, 2],
    "AESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03", "2024-01-01", "2024-01-02"]
  }
}
  ]
}"#,
    )
    .expect("write dataset package");

    load_dataset_package_json(&path)
        .expect("load dataset package")
        .into_iter()
        .next()
        .expect("dataset")
}

pub(super) fn end_date_dataset() -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "se.xpt",
  "domain": "SE",
  "records": {
    "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
    "SESEQ": [1, 2, 3, 1],
    "SESTDTC": ["2024-01-01", "2024-01-02", "2024-01-03", "2024-02-01"],
    "SEENDTC": ["2024-01-02", "", "", ""]
  }
}
  ]
}"#,
    )
    .expect("write dataset package");

    load_dataset_package_json(&path)
        .expect("load dataset package")
        .into_iter()
        .next()
        .expect("dataset")
}

pub(super) fn relationship_dataset() -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "relrec.xpt",
  "domain": "RELREC",
  "records": {
    "USUBJID": ["SUBJ1", "SUBJ1", "SUBJ1", "SUBJ2"],
    "VISITNUM": [1, 2, null, null],
    "RELID": ["R1", "R1", "R2", "R3"],
    "LEFT": ["A", "A", "C", "D"],
    "RIGHT": ["1", "2", "3", "3"],
    "LEFT_EMPTY": ["A", "A", "", "C"],
    "RIGHT_EMPTY": ["1", "", "1", "2"],
    "TARGET_EMPTY_DUP": ["", "", "X", "Y"],
    "GROUP_DUP": ["G", "G", "H", "I"]
  }
}
  ]
}"#,
    )
    .expect("write dataset package");

    load_dataset_package_json(&path)
        .expect("load dataset package")
        .into_iter()
        .next()
        .expect("dataset")
}

pub(super) fn enumerated_dataset() -> LoadedDataset {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "co.xpt",
  "domain": "CO",
  "records": {
    "COSEQ": [1, 2, 3],
    "COVAL": ["primary", "", "primary"],
    "COVAL1": ["", "", "secondary"],
    "COVAL2": ["", "later", ""]
  }
}
  ]
}"#,
    )
    .expect("write dataset package");

    load_dataset_package_json(&path)
        .expect("load dataset package")
        .into_iter()
        .next()
        .expect("dataset")
}

pub(super) fn condition(target: &str, operator: Operator, comparator: ValueExpr) -> Condition {
    Condition {
        target: Some(target.to_owned()),
        operator,
        comparator,
        options: OperatorOptions::default(),
    }
}

pub(super) fn condition_with_options(
    target: &str,
    operator: Operator,
    comparator: ValueExpr,
    options: serde_json::Map<String, Value>,
) -> Condition {
    Condition {
        target: Some(target.to_owned()),
        operator,
        comparator,
        options: OperatorOptions {
            extra: options.into_iter().collect(),
        },
    }
}

pub(super) fn literal(value: impl Into<Value>) -> ValueExpr {
    ValueExpr::Literal(value.into())
}

pub(super) fn rule(
    sensitivity: Option<Sensitivity>,
    conditions: ConditionGroup,
    message: &str,
) -> ExecutableRule {
    ExecutableRule {
        core_id: "CORE-TEST-0001".to_owned(),
        author: None,
        sensitivity,
        executability: None,
        description: None,
        authorities: Vec::new(),
        standards: Vec::new(),
        classes: None,
        domains: None,
        datasets: None,
        entities: None,
        rule_type: RuleType::RecordData,
        conditions,
        actions: vec![ActionSpec {
            name: "generate_dataset_error_objects".to_owned(),
            params: json!({ "message": message }),
        }],
        operations: Vec::new(),
        output_variables: Vec::new(),
        grouping_variables: Vec::new(),
        use_case: None,
        status: None,
        raw: None,
    }
}
