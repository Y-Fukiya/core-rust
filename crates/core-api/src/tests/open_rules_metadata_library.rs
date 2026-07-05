use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};
#[test]
fn run_validation_executes_selected_library_metadata_rules() {
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
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "AESDTH", "label": "Death", "type": "Char", "length": 1 }
      ],
      "records": { "STUDYID": ["S1"], "AESDTH": ["Y"] }
    },
    {
      "filename": "dm.xpt",
      "domain": "DM",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "XRACE", "label": "Race Extension", "type": "Char", "length": 20 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["DM"], "XRACE": ["BLUE"] }
    },
    {
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Distinct Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Blabla", "type": "Char", "length": 8 },
        { "name": "VSORRESU", "label": "Original Units as Collected", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["VS"],
        "USUBJID": ["S1"],
        "VSSEQ": [1],
        "VSTESTCD": ["SYSBP"],
        "VSORRESU": ["mmHg"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
        rules_dir.join("CORE-000398.json"),
        r#"{
  "Core": { "Id": "CORE-000398", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "library_variable_label", "operator": "non_empty" },
    { "name": "variable_label", "operator": "not_equal_to", "value": "library_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable does not correspond to the label in the IG",
    "Output Variables": ["variable_name", "variable_label", "library_variable_label"]
  }
}"#,
    )
    .expect("write label rule");
    fs::write(
            rules_dir.join("CORE-000903.json"),
            r#"{
  "Core": { "Id": "CORE-000903", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["DM", "SE", "CO"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "variable_name", "operator": "exists" },
    { "name": "variable_name", "operator": "not_equal_to", "value": "library_variable_name" }
  ] },
  "Outcome": {
    "Message": "The variable is not allowed in this domain as it is not specified in the SENDIG for the specific domain",
    "Output Variables": ["variable_name"]
  }
}"#,
        )
        .expect("write allowed-variable rule");
    fs::write(
        rules_dir.join("CORE-000507.json"),
        r#"{
  "Core": { "Id": "CORE-000507", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Define XML",
  "Check": { "all": [
    { "name": "variable_label", "operator": "not_equal_to", "value": "define_variable_label" }
  ] },
  "Outcome": {
    "Message": "The label of the variable is incorrect",
    "Output Variables": ["define_variable_name", "define_variable_label", "variable_label"]
  }
}"#,
    )
    .expect("write define label rule");

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
    assert_eq!(failed.len(), 3);
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000398"
        && result.dataset == "AE"
        && result.errors[0].variables
            == vec!["variable_name", "variable_label", "library_variable_label"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000903"
        && result.dataset == "DM"
        && result.errors[0].variables == vec!["variable_name"]));
    assert!(failed.iter().any(|result| result.rule_id == "CORE-000507"
        && result.dataset == "VS"
        && result.error_count == 3));
}

#[test]
fn run_validation_executes_core_000929_domain_codelist_metadata_rule() {
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
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["FA"] }
    },
    {
      "filename": "zb.xpt",
      "domain": "ZB",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 }
      ],
      "records": { "STUDYID": ["S1"], "DOMAIN": ["ZB"] }
    }
  ]
}"#,
    )
    .expect("write datasets");

    fs::write(
            rules_dir.join("CORE-000929.json"),
            r#"{
  "Core": { "Id": "CORE-000929", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$domain_is_custom", "operator": "domain_is_custom" },
    { "id": "$domain_lib_ccode", "operator": "codelist_terms", "codelists": ["DOMAIN"], "returntype": "code" }
  ],
  "Check": { "all": [
    { "name": "$domain_is_custom", "operator": "equal_to", "value": false },
    { "name": "define_variable_ccode", "operator": "equal_to", "value": "C66734" },
    { "name": "define_variable_codelist_coded_codes", "operator": "is_not_contained_by", "value": "$domain_lib_ccode" }
  ] },
  "Outcome": {
    "Message": "DOMAIN Code is not a published DOMAIN Code in CDISC Controlled Terminology.",
    "Output Variables": ["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
  }
}"#,
        )
        .expect("write domain codelist metadata rule");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.FA" Name="FA" Domain="FA">
    <ItemRef ItemOID="IT.FA.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemGroupDef OID="IG.ZB" Name="ZB" Domain="ZB">
    <ItemRef ItemOID="IT.ZB.DOMAIN" OrderNumber="2"/>
  </ItemGroupDef>
  <ItemDef OID="IT.FA.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_FA"/>
  </ItemDef>
  <ItemDef OID="IT.ZB.DOMAIN" Name="DOMAIN">
    <CodeListRef CodeListOID="CL.DOMAIN_ZB"/>
  </ItemDef>
  <CodeList OID="CL.DOMAIN_FA">
    <CodeListItem CodedValue="FA"><Alias Context="nci:ExtCodeID" Name="C00002"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
  <CodeList OID="CL.DOMAIN_ZB">
    <CodeListItem CodedValue="ZB"><Alias Context="nci:ExtCodeID" Name="C49592"/></CodeListItem>
    <Alias Context="nci:ExtCodeID" Name="C66734"/>
  </CodeList>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(data_dir.join(".env"), "VERSION=3-3\n").expect("write env");

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
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000929");
    assert_eq!(failed[0].dataset, "FA");
    assert_eq!(failed[0].error_count, 1);
    assert_eq!(
        failed[0].errors[0].variables,
        vec!["$domain_lib_ccode", "define_variable_codelist_coded_codes"]
    );
}

#[test]
fn run_validation_executes_core_000494_define_role_metadata_rule() {
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
      "filename": "vs.xpt",
      "domain": "VS",
      "variables": [
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 }
      ],
      "records": { "DOMAIN": ["VS"], "VSTESTCD": ["SYSBP"] }
    }
  ]
}"#,
    )
    .expect("write datasets");
    fs::write(
        data_dir.join("define.xml"),
        r#"
<ODM>
  <ItemGroupDef OID="IG.VS" Name="VS" Domain="VS">
    <ItemRef ItemOID="IT.VS.DOMAIN" OrderNumber="2" Role="WRONG: Domain Identifier"/>
    <ItemRef ItemOID="IT.VS.VSTESTCD" OrderNumber="5" Role="Topic"/>
  </ItemGroupDef>
  <ItemDef OID="IT.VS.DOMAIN" Name="DOMAIN">
    <Description><TranslatedText>Domain Abbreviation</TranslatedText></Description>
  </ItemDef>
  <ItemDef OID="IT.VS.VSTESTCD" Name="VSTESTCD">
    <Description><TranslatedText>Vital Signs Test Short Name</TranslatedText></Description>
  </ItemDef>
</ODM>
"#,
    )
    .expect("write define xml");
    fs::write(
            rules_dir.join("CORE-000494.json"),
            r#"{
  "Core": { "Id": "CORE-000494", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Define Item Metadata Check against Library Metadata",
  "Check": { "all": [
    { "name": "define_variable_name", "operator": "equal_to", "value": "library_variable_name" },
    { "name": "define_variable_role", "operator": "not_equal_to", "value": "library_variable_role" }
  ] },
  "Outcome": {
    "Message": "The Role of the variable in the define.xml does not correspond to the Role given by the Implementation Guide",
    "Output Variables": [
      "define_variable_label",
      "define_variable_name",
      "define_variable_role",
      "library_variable_name",
      "library_variable_role"
    ]
  }
}"#,
        )
        .expect("write rule");

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
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].rule_id, "CORE-000494");
    assert_eq!(failed[0].dataset, "VS");
    assert_eq!(failed[0].errors[0].row, Some(1));
    assert_eq!(
        failed[0].errors[0].variables,
        vec![
            "define_variable_label",
            "define_variable_name",
            "define_variable_role",
            "library_variable_name",
            "library_variable_role"
        ]
    );
}

#[test]
fn run_validation_executes_core_000595_missing_casno_oracle_issue() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
            rules_dir.join("CORE-000595.json"),
            r#"{
  "Core": { "Id": "CORE-000595", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["IN"] }, "Classes": { "Include": ["SPECIAL PURPOSE"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Check": { "any": [
    { "all": [
      { "name": "UNII", "operator": "empty" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "not_exists" }
    ] },
    { "all": [
      { "name": "UNII", "operator": "not_exists" },
      { "name": "CASNO", "operator": "empty" }
    ] },
    { "all": [
      { "name": "CASNO", "operator": "not_exists" },
      { "name": "UNII", "operator": "empty" }
    ] }
  ] },
  "Outcome": {
    "Message": "At least one of the UNII and CASNO variables should be present and populated for each ingredient if available.",
    "Output Variables": ["UNII", "CASNO"]
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
      "filename": "in.xpt",
      "domain": "IN",
      "class": "SPECIAL PURPOSE",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 12 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "UNII", "label": "Unique Ingredient Identifier", "type": "Char", "length": 50 }
      ],
      "records": {
        "STUDYID": ["TOB07"],
        "DOMAIN": ["IN"],
        "UNII": ["UNI2"]
      }
    }
  ]
}"#,
    )
    .expect("write datasets");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        open_rules_oracle_compat: true,
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, None);
    assert!(outcome.results[0].errors[0].variables.is_empty());
}

#[test]
fn run_validation_executes_send_variable_metadata_model_column_order_rule() {
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
      "filename": "vs.xpt",
      "domain": "VS",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "VSSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "VSTESTCD", "label": "Vital Signs Test Short Name", "type": "Char", "length": 8 },
        { "name": "VSTEST", "label": "Vital Signs Test Name", "type": "Char", "length": 40 },
        { "name": "VSNONSEN", "label": "Vital Signs Nonsense", "type": "Char", "length": 40 },
        { "name": "VSORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "VSNOTDY", "label": "Non Study Day of Vital Signs", "type": "Num", "length": 8 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["VS"],
        "USUBJID": ["01"],
        "VSSEQ": [1],
        "VSTESTCD": ["WEIGHT"],
        "VSTEST": ["Weight"],
        "VSNONSEN": ["bad"],
        "VSORRES": ["80"],
        "VSNOTDY": [1]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
        rules_dir.join("CORE-000902.json"),
        r#"{
  "Core": { "Id": "CORE-000902", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check against Library Metadata",
  "Operations": [
    { "id": "$allowed_variables", "operator": "get_model_column_order" }
  ],
  "Check": {
    "all": [
      { "name": "variable_name", "operator": "is_not_contained_by", "value": "$allowed_variables" }
    ]
  },
  "Outcome": {
    "Message": "The variable is not an allowed variable for the underlying Observation Class",
    "Output Variables": ["variable_name"]
  }
}"#,
    )
    .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 2);
    let rows = outcome.results[0]
        .errors
        .iter()
        .map(|error| error.row)
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![Some(7), Some(9)]);
}

#[test]
fn run_validation_executes_custom_domain_variable_prefix_metadata_rule() {
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
      "filename": "zb.xpt",
      "domain": "ZB",
      "class": "FINDINGS",
      "variables": [
        { "name": "STUDYID", "label": "Study Identifier", "type": "Char", "length": 10 },
        { "name": "DOMAIN", "label": "Domain Abbreviation", "type": "Char", "length": 2 },
        { "name": "USUBJID", "label": "Unique Subject Identifier", "type": "Char", "length": 20 },
        { "name": "ZBSEQ", "label": "Sequence Number", "type": "Num", "length": 8 },
        { "name": "LBORRES", "label": "Result or Finding in Original Units", "type": "Char", "length": 20 },
        { "name": "ZBORRESU", "label": "Original Units", "type": "Char", "length": 20 }
      ],
      "records": {
        "STUDYID": ["CDISC-TEST"],
        "DOMAIN": ["ZB"],
        "USUBJID": ["01"],
        "ZBSEQ": [1],
        "LBORRES": ["80"],
        "ZBORRESU": ["kg"]
      }
    }
  ]
}"#,
        )
        .expect("write datasets");
    fs::write(
            rules_dir.join("CORE-000376.json"),
            r#"{
  "Core": { "Id": "CORE-000376", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["FINDINGS"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Operations": [
    { "id": "$domain_list", "name": "DOMAIN", "operator": "distinct" },
    { "id": "$domain_is_custom", "operator": "domain_is_custom" }
  ],
  "Check": {
    "all": [
      { "name": "$domain_is_custom", "operator": "equal_to", "value": true },
      { "name": "variable_name", "operator": "is_not_contained_by", "value": ["STUDYID", "DOMAIN", "USUBJID"] },
      { "name": "variable_name", "operator": "prefix_is_not_contained_by", "prefix": 2, "value": "$domain_list" }
    ]
  },
  "Outcome": {
    "Message": "First 2 characters of prefixed variable within custom domain do not match the DOMAIN value.",
    "Output Variables": ["$domain_list", "variable_name"]
  }
}"#,
        )
        .expect("write metadata rule");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].dataset, "ZB");
    assert_eq!(outcome.results[0].errors[0].row, Some(5));
}
