use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
