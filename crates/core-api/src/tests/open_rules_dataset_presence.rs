use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

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
