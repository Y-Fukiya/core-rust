use std::fs;

use core_engine::ExecutionStatus;
use core_rule_model::load_rules_from_paths;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::operation_fields::operation_name;
use crate::{run_validation, DatasetLoader, ValidateRequest};

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
