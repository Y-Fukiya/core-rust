use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

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
