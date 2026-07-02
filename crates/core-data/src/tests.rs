use std::fs;
use std::fs::File;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;

#[test]
fn load_csv_dataset_builds_metadata_and_summary() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("AE.csv");
    fs::write(
        &path,
        "STUDYID,DOMAIN,AESEQ\nCDISC-TEST,AE,1\nCDISC-TEST,AE,2\n",
    )
    .expect("write csv");

    let dataset = load_csv_dataset(&path).expect("load csv");
    let summary = dataset.summary();

    assert_eq!(dataset.metadata().name, "AE");
    assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
    assert_eq!(summary.filename, "AE.csv");
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.columns, vec!["STUDYID", "DOMAIN", "AESEQ"]);
    assert_eq!(dataset.frame().height(), 2);
}

#[test]
fn load_csv_dataset_preserves_leading_zero_codes_as_strings() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("AE.csv");
    fs::write(&path, "CODE,COUNT\n01,1\n001,2\n").expect("write csv");

    let dataset = load_csv_dataset(&path).expect("load csv");
    let code = dataset.frame().column("CODE").expect("code column");
    let count = dataset.frame().column("COUNT").expect("count column");

    assert_eq!(code.get(0).expect("row 1").extract_str(), Some("01"));
    assert_eq!(code.get(1).expect("row 2").extract_str(), Some("001"));
    assert_eq!(count.get(0).expect("row 1"), AnyValue::Int64(1));
}

#[test]
fn load_open_rules_data_dir_uses_variables_schema() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\ncm,Concomitant Medications\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\nCM,CMTRT,Treatment,Char,40\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("cm.csv"),
        "CMSEQ,CMTRT\n001,ASPIRIN\n,PLACEBO\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert!(result.warnings.is_empty());
    let dataset = &result.datasets[0];
    assert_eq!(dataset.metadata.name, "CM");
    assert_eq!(dataset.metadata.domain.as_deref(), Some("CM"));
    assert_eq!(
        dataset.metadata.label.as_deref(),
        Some("Concomitant Medications")
    );
    assert_eq!(dataset.metadata.variables[0].name, "CMSEQ");
    assert_eq!(
        dataset.metadata.variables[0].variable_type.as_deref(),
        Some("Num")
    );
    assert_eq!(
        dataset_column_values(dataset, "CMSEQ").expect("CMSEQ values"),
        vec![serde_json::json!(1), serde_json::Value::Null]
    );
    assert_eq!(
        dataset_column_values(dataset, "CMTRT").expect("CMTRT values"),
        vec![serde_json::json!("ASPIRIN"), serde_json::json!("PLACEBO")]
    );
}

#[test]
fn load_open_rules_data_dir_preserves_declared_variable_name_case_for_metadata_rules() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\ndm,Demographics\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\n\
dm,STUDYID,Study Identifier,Char,50\n\
dm,aDOMAIN,Domain Abbreviation,Char,50\n\
dm,ARM_NRS,Reason Arm and/or Actual Arm is Null,Char,50\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("dm.csv"),
        "STUDYID,aDOMAIN,ARM_NRS\nABC,DM,SCREEN FAILURE\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    let variable_names = result.datasets[0]
        .metadata
        .variables
        .iter()
        .map(|variable| variable.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(variable_names, vec!["STUDYID", "aDOMAIN", "ARM_NRS"]);
    assert_eq!(
        dataset_column_values(&result.datasets[0], "ADOMAIN").expect("canonical data column"),
        vec![serde_json::json!("DM")]
    );
}

#[test]
fn load_open_rules_data_dir_maps_csv_label_headers_to_variable_names() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\nec,Exposure as Collected\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\n\
ec,STUDYID,Study Identifier,Char,50\n\
ec,DOMAIN,Domain Abbreviation,Char,50\n\
ec,USUBJID,Unique Subject Identifier,Char,50\n\
ec,ECSEQ,Sequence Number,Num,8\n\
ec,ECDOSE,Dose,Num,8\n\
ec,ECDOSTXT,Dose Description,Char,50\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("ec.csv"),
        "Study Identifier,Domain Abbreviation,Unique Subject Identifier,Sequence Number,Dose,Dose Description\n\
ABC,EC,ABC1001,1,2,\n\
ABC,EC,ABC2001,2,2,2-3\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert!(result.warnings.is_empty());
    let dataset = &result.datasets[0];
    assert_eq!(
        dataset_column_values(dataset, "ECDOSE").expect("ECDOSE values"),
        vec![serde_json::json!(2), serde_json::json!(2)]
    );
    assert_eq!(
        dataset_column_values(dataset, "ECDOSTXT").expect("ECDOSTXT values"),
        vec![serde_json::json!(""), serde_json::json!("2-3")]
    );
    assert!(dataset.frame().column("Dose").is_err());
}

#[test]
fn load_open_rules_data_dir_skips_leading_metadata_rows_before_label_headers() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\nec,Exposure as Collected\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\n\
ec,STUDYID,Study Identifier,Char,50\n\
ec,DOMAIN,Domain Abbreviation,Char,50\n\
ec,USUBJID,Unique Subject Identifier,Char,50\n\
ec,ECSEQ,Sequence Number,Num,8\n\
ec,ECDOSE,Dose,Num,8\n\
ec,ECDOSTXT,Dose Description,Char,50\n\
ec,ECDOSU,Dose Units,Char,50\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("ec.csv"),
        ",,,,,,\n\
Study Identifier,Domain Abbreviation,Unique Subject Identifier,Sequence Number,Dose,Dose Description,Dose Units\n\
Char,Char,Char,Num,Num,Char,Char\n\
50,50,50,8,8,50,50\n\
ABC,EC,ABC2001,1,2,,\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert!(result.warnings.is_empty());
    let dataset = &result.datasets[0];
    assert_eq!(dataset.frame().height(), 1);
    assert_eq!(
        dataset_column_values(dataset, "ECDOSE").expect("ECDOSE values"),
        vec![serde_json::json!(2)]
    );
    assert_eq!(
        dataset_column_values(dataset, "ECDOSU").expect("ECDOSU values"),
        vec![serde_json::json!("")]
    );
    assert!(dataset.frame().column("Dose").is_err());
}

#[test]
fn load_open_rules_data_dir_infers_missing_dataset_schema_from_embedded_metadata_rows() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\nec,Exposure as Collected\nex,Exposure\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\n\
ex,STUDYID,Study Identifier,Char,50\n\
ex,DOMAIN,Domain Abbreviation,Char,50\n\
ex,USUBJID,Unique Subject Identifier,Char,50\n\
ex,EXSEQ,Sequence Number,Num,8\n\
ex,EXDOSE,Dose,Num,8\n\
ex,EXDOSTXT,Dose Description,Char,50\n\
ex,EXDOSU,Dose Units,Char,50\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("ec.csv"),
        ",,,,,,\n\
Study Identifier,Domain Abbreviation,Unique Subject Identifier,Sequence Number,Dose,Dose Description,Dose Units\n\
Char,Char,Char,Num,Num,Char,Char\n\
50,50,50,8,8,50,50\n\
ABC,EC,ABC2001,1,2,,\n",
    )
    .expect("write ec dataset csv");
    fs::write(
        dir.path().join("ex.csv"),
        "STUDYID,DOMAIN,USUBJID,EXSEQ,EXDOSE,EXDOSTXT,EXDOSU\n\
ABC,EX,ABC1001,1,2,,TABLET\n",
    )
    .expect("write ex dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    let dataset = result
        .datasets
        .iter()
        .find(|dataset| dataset.metadata.name == "EC")
        .expect("EC dataset");
    assert_eq!(
        dataset_column_values(dataset, "ECDOSE").expect("ECDOSE values"),
        vec![serde_json::json!(2)]
    );
    assert_eq!(
        dataset_column_values(dataset, "ECDOSU").expect("ECDOSU values"),
        vec![serde_json::json!("")]
    );
    assert!(dataset.frame().column("Dose").is_err());
}

#[test]
fn load_open_rules_data_dir_preserves_variable_label_trailing_space() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\nml,Meals\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nml,USUBJID,Unique Subject Identifier Unique Subject ,Char,50\n",
    )
    .expect("write variables csv");
    fs::write(dir.path().join("ml.csv"), "USUBJID\nSUBJ1\n").expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    let label = result.datasets[0].metadata.variables[0]
        .label
        .as_deref()
        .expect("variable label");
    assert_eq!(label, "Unique Subject Identifier Unique Subject ");
    assert_eq!(label.chars().count(), 41);
}

#[test]
fn load_open_rules_data_dir_uses_horizontal_variables_schema() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\nvs,Vital Signs\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "STUDYID,DOMAIN,USUBJID,VSSEQ,VSTESTCD\n\
Study Identifier,Domain Abbreviation,Unique Subject Identifier,Sequence Number,Vital Signs Test Short Name\n\
Char,Char,Char,Num,Char\n\
12,2,8,8,12\n",
    )
    .expect("write horizontal variables csv");
    fs::write(
        dir.path().join("vs.csv"),
        "STUDYID,DOMAIN,USUBJID,VSSEQ,VSTESTCD\n\
CDISCPILOT01,VS,CDISC001,1,DIABP_1\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert!(result.warnings.is_empty());
    let dataset = &result.datasets[0];
    assert_eq!(dataset.metadata.name, "VS");
    assert_eq!(dataset.metadata.variables[3].name, "VSSEQ");
    assert_eq!(
        dataset.metadata.variables[3].label.as_deref(),
        Some("Sequence Number")
    );
    assert_eq!(
        dataset.metadata.variables[3].variable_type.as_deref(),
        Some("Num")
    );
    assert_eq!(dataset.metadata.variables[3].length, Some(8));
    assert_eq!(
        dataset_column_values(dataset, "VSSEQ").expect("VSSEQ values"),
        vec![serde_json::json!(1)]
    );
}

#[test]
fn load_open_rules_data_dir_rejects_dataset_filename_parent_dir() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\n../outside,Outside\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nOUTSIDE,USUBJID,Subject,Char,20\n",
    )
    .expect("write variables csv");

    let error = load_open_rules_data_dir(dir.path()).expect_err("unsafe filename rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("unsafe dataset filename"))
    );
}

#[test]
fn load_open_rules_data_dir_rejects_dataset_filename_absolute_path() {
    let dir = tempdir().expect("tempdir");
    let outside = dir.path().join("outside.csv");
    fs::write(&outside, "USUBJID\n01\n").expect("write outside csv");
    fs::write(
        dir.path().join("_datasets.csv"),
        format!("Filename,Label\n{},Outside\n", outside.display()),
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nOUTSIDE,USUBJID,Subject,Char,20\n",
    )
    .expect("write variables csv");

    let error = load_open_rules_data_dir(dir.path()).expect_err("absolute filename rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("unsafe dataset filename"))
    );
}

#[test]
fn load_open_rules_data_dir_uses_dataset_name_manifest_column() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Dataset Name,Label\nStudyProtocolDocumentVersio,StudyProtocolDocumentVersion,Study Protocol Version\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nStudyProtocolDocumentVersio,id,Identifier,String,[1]\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("StudyProtocolDocumentVersio.csv"),
        "id\nStudyProtocolDocumentVersion_1\n",
    )
    .expect("write dataset csv");

    let datasets = load_open_rules_data_dir(dir.path()).expect("load open rules data");

    assert_eq!(datasets.len(), 1);
    let dataset = &datasets[0];
    assert_eq!(dataset.metadata.name, "STUDYPROTOCOLDOCUMENTVERSION");
    assert_eq!(
        dataset.metadata.domain.as_deref(),
        Some("STUDYPROTOCOLDOCUMENTVERSION")
    );
    assert_eq!(dataset.metadata.variables[0].name, "ID");
}

#[test]
fn load_open_rules_data_dir_warns_for_schema_mismatches() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("_datasets.csv"), "Filename,Label\ncm,CM\n")
        .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\nCM,MISSING,Missing,Char,20\n",
    )
    .expect("write variables csv");
    fs::write(dir.path().join("cm.csv"), "CMSEQ,EXTRA\nabc,value\n").expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.warnings.len(), 3);
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::InvalidNumericValue { .. }
    )));
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::DeclaredVariableMissing { .. }
    )));
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::UndeclaredCsvColumn { .. }
    )));
}

#[test]
fn load_open_rules_data_dir_ignores_empty_trailing_csv_columns() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("_datasets.csv"), "Filename,Label\nae,AE\n")
        .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nAE,STUDYID,Study Identifier,Char,50\nAE,DOMAIN,Domain Abbreviation,Char,50\nAE,AESEQ,Sequence Number,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("ae.csv"),
        "STUDYID,DOMAIN,AESEQ,,\nS1,AE,1,,\nS2,AE,2\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(
        result.datasets[0].summary().columns,
        vec!["STUDYID", "DOMAIN", "AESEQ"]
    );
    assert_eq!(result.datasets[0].summary().row_count, 2);
    assert!(result.warnings.is_empty());
}

#[test]
fn load_open_rules_data_dir_ignores_blank_dataset_manifest_rows() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\n,\nae,Adverse Events\n",
    )
    .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nAE,STUDYID,Study Identifier,Char,50\nAE,AESEQ,Sequence Number,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(dir.path().join("ae.csv"), "STUDYID,AESEQ\nS1,1\n").expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.datasets[0].metadata().name, "AE");
}

#[test]
fn load_open_rules_data_dir_ignores_duplicate_csv_columns() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("_datasets.csv"), "Filename,Label\ndm,DM\n")
        .expect("write datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nDM,ACTARMCD,Actual Arm Code,Char,8\n",
    )
    .expect("write variables csv");
    fs::write(
        dir.path().join("dm.csv"),
        "ACTARMCD,ACTARMCD\nARM-A,duplicate\nARM-B,duplicate\n",
    )
    .expect("write dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.datasets[0].summary().columns, vec!["ACTARMCD"]);
    assert_eq!(
        dataset_column_values(&result.datasets[0], "ACTARMCD").expect("ACTARMCD values"),
        vec![serde_json::json!("ARM-A"), serde_json::json!("ARM-B")]
    );
}

#[test]
fn load_open_rules_data_dir_infers_dataset_manifest_when_missing() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nPD,PDSEQ,Sequence Number,Num,8\nPD,PDVALTRG,Target,Num,8\n",
    )
    .expect("write variables csv");
    fs::write(dir.path().join("pd.csv"), "PDSEQ,PDVALTRG\n1,10\n").expect("write dataset csv");
    fs::write(
        dir.path().join("results.csv"),
        "Dataset,Record,Variable,Value\n",
    )
    .expect("write stray results csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.datasets[0].metadata.name, "PD");
    assert_eq!(result.datasets[0].metadata.filename, "pd.csv");
    assert_eq!(
        dataset_column_values(&result.datasets[0], "PDVALTRG").expect("PDVALTRG values"),
        vec![serde_json::json!(10)]
    );
}

#[test]
fn load_open_rules_data_dir_creates_empty_json_schema_issue_dataset_without_schema() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join(".env"), "PRODUCT=SDTMIG\n").expect("write env");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.datasets[0].metadata.name, "JSONSchemaIssue");
    assert_eq!(result.datasets[0].frame().height(), 0);
    assert!(result.warnings.is_empty());
}

#[test]
fn load_open_rules_data_dir_records_usdm_planned_sex_max_items_schema_issue() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("usdm.json"),
        r#"{
  "study": {
"versions": [
  {
    "studyDesigns": [
      {
        "population": {
          "plannedSex": [
            { "id": "Code_1", "code": "C49636" },
            { "id": "Code_2", "code": "C20197" }
          ]
        }
      }
    ]
  }
]
  }
}"#,
    )
    .expect("write json");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");
    let dataset = result
        .datasets
        .iter()
        .find(|dataset| dataset.metadata().name == "JSONSchemaIssue")
        .expect("JSONSchemaIssue dataset");

    assert_eq!(dataset.summary().row_count, 1);
    assert_eq!(
        dataset_column_values(dataset, "path").expect("path values"),
        vec![serde_json::json!(
            "/study/versions/0/studyDesigns/0/population"
        )]
    );
    assert_eq!(
        dataset_column_values(dataset, "validator").expect("validator values"),
        vec![serde_json::json!("maxItems")]
    );
    assert_eq!(
        dataset_column_values(dataset, "error_attribute").expect("attribute values"),
        vec![serde_json::json!("plannedSex")]
    );
}

#[test]
fn load_open_rules_data_dir_flattens_usdm_study_design_population_json() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("usdm.json"),
        r#"{
  "study": {
"versions": [
  {
    "studyDesigns": [
      {
        "id": "StudyDesign_1",
        "name": "Main Design",
        "population": {
          "id": "Population_1",
          "name": "POP1",
          "instanceType": "StudyDesignPopulation",
          "plannedEnrollmentNumber": {
            "id": "Quantity_1",
            "value": 22,
            "unit": {
              "id": "Unit_1",
              "standardCode": { "decode": "Day", "code": "C25301" }
            }
          },
          "cohorts": [
            {
              "id": "StudyCohort_1",
              "name": "COHORT1",
              "plannedEnrollmentNumber": { "id": "Quantity_2", "value": 10 }
            }
          ]
        }
      }
    ]
  }
]
  }
}"#,
    )
    .expect("write json");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");
    let dataset = result
        .datasets
        .iter()
        .find(|dataset| dataset.metadata().name == "StudyDesignPopulation")
        .expect("StudyDesignPopulation dataset");

    assert_eq!(dataset.summary().row_count, 1);
    assert_eq!(
        dataset_column_values(dataset, "StudyDesign.id").expect("StudyDesign.id"),
        vec![serde_json::json!("StudyDesign_1")]
    );
    assert_eq!(
        dataset_column_values(dataset, "plannedEnrollmentNumber.has_unit").expect("has unit"),
        vec![serde_json::json!(true)]
    );
}

#[test]
fn load_open_rules_data_dir_formats_usdm_address_jsonata_rep_values() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("usdm.json"),
        r#"{
  "study": {
"versions": [
  {
    "organizations": [
      {
        "id": "Organization_1",
        "name": "Missing",
        "legalAddress": {
          "id": "Address_1",
          "instanceType": "Address"
        }
      },
      {
        "id": "Organization_2",
        "name": "Nulls",
        "legalAddress": {
          "id": "Address_2",
          "instanceType": "Address",
          "text": null,
          "lines": [],
          "city": null,
          "district": null,
          "state": null,
          "postalCode": null,
          "country": null
        }
      },
      {
        "id": "Organization_3",
        "name": "Country",
        "legalAddress": {
          "id": "Address_3",
          "instanceType": "Address",
          "text": null,
          "lines": [],
          "city": null,
          "district": null,
          "state": null,
          "postalCode": null,
          "country": { "code": "GBR", "decode": "United Kingdom" }
        }
      }
    ]
  }
]
  }
}"#,
    )
    .expect("write json");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");
    let dataset = result
        .datasets
        .iter()
        .find(|dataset| dataset.metadata().name == "Address")
        .expect("Address dataset");

    assert_eq!(
        dataset_column_values(dataset, "text").expect("text values"),
        vec![
            serde_json::json!("Missing"),
            serde_json::Value::Null,
            serde_json::Value::Null
        ]
    );
    assert_eq!(
        dataset_column_values(dataset, "lines").expect("lines values"),
        vec![
            serde_json::json!("Missing"),
            serde_json::json!("[]"),
            serde_json::json!("[]")
        ]
    );
    assert_eq!(
        dataset_column_values(dataset, "city").expect("city values"),
        vec![
            serde_json::json!("Missing"),
            serde_json::Value::Null,
            serde_json::Value::Null
        ]
    );
    assert_eq!(
        dataset_column_values(dataset, "address_all_blank").expect("blank flags"),
        vec![
            serde_json::json!(true),
            serde_json::json!(true),
            serde_json::json!(false)
        ]
    );
}

#[test]
fn generic_csv_loader_still_infers_values() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("cm.csv");
    fs::write(&path, "CMSEQ\n001\n").expect("write csv");

    let dataset = load_csv_dataset(&path).expect("load csv");

    assert_eq!(
        dataset_column_values(&dataset, "CMSEQ").expect("CMSEQ values"),
        vec![serde_json::json!("001")]
    );
}

#[test]
fn load_dataset_package_json_builds_datasets() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "ae.xpt",
  "label": "Adverse Events",
  "domain": "AE",
  "variables": [
    {
      "name": "STUDYID",
      "label": "Study Identifier",
      "type": "Char",
      "length": 10
    },
    {
      "name": "AESEQ",
      "label": "Sequence Number",
      "type": "Num",
      "length": 8
    }
  ],
  "records": {
    "STUDYID": ["CDISC-TEST", "CDISC-TEST"],
    "DOMAIN": ["AE", "AE"],
    "AESEQ": [1, 2]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let dataset = &datasets[0];
    let summary = dataset.summary();

    assert_eq!(datasets.len(), 1);
    assert_eq!(dataset.metadata().name, "AE");
    assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
    assert_eq!(dataset.metadata().label.as_deref(), Some("Adverse Events"));
    assert_eq!(dataset.metadata().filename, "ae.xpt");
    assert_eq!(dataset.metadata().variables.len(), 2);
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.columns, vec!["STUDYID", "DOMAIN", "AESEQ"]);
}

#[test]
fn load_xpt_dataset_builds_metadata_and_rows() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("STUDYID", 12, "Study Identifier"),
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric("AESEQ", "Sequence Number"),
        ],
        &[
            vec![
                TestXptValue::Text("CDISC-TEST"),
                TestXptValue::Text("AE"),
                TestXptValue::Number(1.0),
            ],
            vec![
                TestXptValue::Text("CDISC-TEST"),
                TestXptValue::Text("AE"),
                TestXptValue::Number(2.0),
            ],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let summary = dataset.summary();

    assert_eq!(dataset.metadata().name, "AE");
    assert_eq!(dataset.metadata().domain.as_deref(), Some("AE"));
    assert_eq!(dataset.metadata().source_format, DatasetSourceFormat::Xpt);
    assert_eq!(dataset.metadata().variables.len(), 3);
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.columns, vec!["STUDYID", "DOMAIN", "AESEQ"]);
    assert_eq!(
        dataset
            .frame()
            .column("DOMAIN")
            .expect("domain column")
            .get(0)
            .expect("row 1")
            .extract_str(),
        Some("AE")
    );
    assert_eq!(
        dataset
            .frame()
            .column("AESEQ")
            .expect("seq column")
            .get(1)
            .expect("row 2"),
        AnyValue::Int64(2)
    );
}

#[test]
fn load_xpt_dataset_rejects_oversized_file_before_reading() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("huge.xpt");
    let file = File::create(&path).expect("create xpt");
    file.set_len(XPT_MAX_FILE_BYTES as u64 + 1)
        .expect("set sparse len");
    drop(file);

    let error = load_xpt_dataset(&path).expect_err("oversized xpt rejected");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("exceeds maximum supported size"))
    );
}

#[test]
fn load_xpt_dataset_preserves_zero_numeric_values() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric("AESEQ", "Sequence Number"),
        ],
        &[
            vec![TestXptValue::Text("AE"), TestXptValue::Number(0.0)],
            vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let seq = dataset.frame().column("AESEQ").expect("seq column");

    assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(0));
    assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(1));
}

#[test]
fn load_xpt_dataset_decodes_short_numeric_lengths() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("ae.xpt");
    write_test_xpt(
        &path,
        "AE",
        &[
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
            TestXptVariable::numeric_with_length("AESEQ", 4, "Sequence Number"),
        ],
        &[
            vec![TestXptValue::Text("AE"), TestXptValue::Number(1.0)],
            vec![TestXptValue::Text("AE"), TestXptValue::Number(2.0)],
        ],
    );

    let dataset = load_xpt_dataset(&path).expect("load xpt");
    let seq = dataset.frame().column("AESEQ").expect("seq column");

    assert_eq!(dataset.metadata().variables[1].length, Some(4));
    assert_eq!(seq.get(0).expect("row 1"), AnyValue::Int64(1));
    assert_eq!(seq.get(1).expect("row 2"), AnyValue::Int64(2));
}

#[test]
fn load_datasets_from_directory_scans_direct_children_only() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("AE.csv"), "STUDYID,DOMAIN\nS1,AE\n").expect("write csv");
    fs::write(
        dir.path().join("package.json"),
        r#"{
  "datasets": [
{
  "filename": "cm.xpt",
  "domain": "CM",
  "records": {
    "STUDYID": ["S1"],
    "DOMAIN": ["CM"]
  }
}
  ]
}"#,
    )
    .expect("write package");
    fs::write(dir.path().join("notes.txt"), "ignore me").expect("write notes");
    write_test_xpt(
        &dir.path().join("VS.xpt"),
        "VS",
        &[
            TestXptVariable::character("STUDYID", 8, "Study Identifier"),
            TestXptVariable::character("DOMAIN", 2, "Domain Abbreviation"),
        ],
        &[vec![TestXptValue::Text("S1"), TestXptValue::Text("VS")]],
    );

    let nested = dir.path().join("nested");
    fs::create_dir(&nested).expect("create nested");
    fs::write(nested.join("VS.csv"), "STUDYID,DOMAIN\nS1,VS\n").expect("write nested csv");

    let result = load_datasets_from_paths_with_warnings(&[dir.path().to_path_buf()])
        .expect("load directory");

    assert_eq!(result.datasets.len(), 3);
    assert_eq!(
        dataset_names(&result.datasets),
        BTreeSet::from(["AE".to_owned(), "CM".to_owned(), "VS".to_owned()])
    );
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(
        result.warnings[0].kind,
        LoadDataWarningKind::UnsupportedExtension("txt".to_owned())
    );
}

#[test]
fn package_json_rejects_mismatched_record_lengths() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("bad.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "ae.xpt",
  "domain": "AE",
  "records": {
    "STUDYID": ["S1", "S2"],
    "DOMAIN": ["AE"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let error = load_dataset_package_json(&path).expect_err("mismatched lengths fail");

    assert!(matches!(error, DataError::Polars { .. }));
}

#[test]
fn left_join_dataset_adds_prefixed_right_columns() {
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
    "USUBJID": ["S1", "S2", "S3"],
    "DOMAIN": ["AE", "AE", "AE"],
    "AESEQ": [1, 2, 3]
  }
},
{
  "filename": "suppae.xpt",
  "domain": "SUPPAE",
  "records": {
    "USUBJID": ["S1", "S3"],
    "QNAM": ["AESPID", "AESPID"],
    "QVAL": ["A", "C"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let joined = left_join_dataset(
        &datasets[0],
        &datasets[1],
        &["USUBJID".to_owned()],
        "SUPPAE.",
    )
    .expect("join datasets");

    assert_eq!(
        joined.summary().columns,
        vec!["USUBJID", "DOMAIN", "AESEQ", "SUPPAE.QNAM", "SUPPAE.QVAL"]
    );
    assert_eq!(
        joined
            .frame()
            .column("SUPPAE.QVAL")
            .expect("joined QVAL")
            .get(0)
            .expect("row 1")
            .extract_str(),
        Some("A")
    );
    assert!(joined
        .frame()
        .column("SUPPAE.QVAL")
        .expect("joined QVAL")
        .get(1)
        .expect("row 2")
        .is_null());
}

#[test]
fn left_join_dataset_on_allows_different_left_and_right_key_names() {
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
    "USUBJID": ["S1", "S2"],
    "DOMAIN": ["AE", "AE"]
  }
},
{
  "filename": "lookup.json",
  "domain": "LOOKUP",
  "records": {
    "SUBJECT": ["S2"],
    "FLAG": ["Y"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let joined = left_join_dataset_on(
        &datasets[0],
        &datasets[1],
        &["usubjid".to_owned()],
        &["subject".to_owned()],
        "LOOKUP.",
    )
    .expect("join datasets");

    assert_eq!(
        joined.summary().columns,
        vec!["USUBJID", "DOMAIN", "LOOKUP.FLAG"]
    );
    assert!(joined
        .frame()
        .column("LOOKUP.FLAG")
        .expect("joined flag")
        .get(0)
        .expect("row 1")
        .is_null());
    assert_eq!(
        joined
            .frame()
            .column("LOOKUP.FLAG")
            .expect("joined flag")
            .get(1)
            .expect("row 2")
            .extract_str(),
        Some("Y")
    );
}

#[test]
fn left_join_dataset_on_keeps_unprefixed_different_right_key_name() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "epoch.json",
  "domain": "StudyEpoch",
  "records": {
    "id": ["StudyEpoch_1", "StudyEpoch_2"]
  }
},
{
  "filename": "activity.json",
  "domain": "ScheduledActivityInstance",
  "records": {
    "epochId": ["StudyEpoch_1"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let joined = left_join_dataset_on(
        &datasets[0],
        &datasets[1],
        &["id".to_owned()],
        &["epochId".to_owned()],
        "",
    )
    .expect("join datasets");

    assert_eq!(joined.summary().columns, vec!["id", "epochId"]);
    assert_eq!(
        joined
            .frame()
            .column("epochId")
            .expect("joined right key")
            .get(0)
            .expect("row 1")
            .extract_str(),
        Some("StudyEpoch_1")
    );
    assert!(joined
        .frame()
        .column("epochId")
        .expect("joined right key")
        .get(1)
        .expect("row 2")
        .is_null());
}

#[test]
fn joins_fan_out_duplicate_right_keys_and_preserve_value_types() {
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
    "USUBJID": ["S1", "S2"],
    "AESEQ": [1, 2]
  }
},
{
  "filename": "lookup.json",
  "domain": "LOOKUP",
  "records": {
    "USUBJID": ["S1", "S1"],
    "FLAGN": [10, 20]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let keys = ["USUBJID".to_owned()];
    let left = left_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys, "LOOKUP.")
        .expect("left join");
    let inner = inner_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys, "LOOKUP.")
        .expect("inner join");
    let joined_flag = left.frame().column("LOOKUP.FLAGN").expect("joined flag");

    assert_eq!(left.summary().row_count, 3);
    assert_eq!(inner.summary().row_count, 2);
    assert_eq!(joined_flag.get(0).expect("row 1"), AnyValue::Int64(10));
    assert_eq!(joined_flag.get(1).expect("row 2"), AnyValue::Int64(20));
    assert!(joined_flag.get(2).expect("row 3").is_null());
}

#[test]
fn join_variants_filter_rows_by_match_presence() {
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
    "USUBJID": ["S1", "S2", "S3"],
    "AESEQ": [1, 2, 3]
  }
},
{
  "filename": "lookup.json",
  "domain": "LOOKUP",
  "records": {
    "USUBJID": ["S2", "S3"],
    "FLAG": ["Y", "N"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let keys = ["USUBJID".to_owned()];
    let inner = inner_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys, "LOOKUP.")
        .expect("inner join");
    let semi = semi_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys).expect("semi join");
    let anti = anti_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys).expect("anti join");

    assert_eq!(inner.summary().row_count, 2);
    assert_eq!(semi.summary().row_count, 2);
    assert_eq!(anti.summary().row_count, 1);
    assert_eq!(
        anti.frame()
            .column("USUBJID")
            .expect("subject")
            .get(0)
            .expect("anti row")
            .extract_str(),
        Some("S1")
    );
}

#[test]
fn join_keys_respect_value_types() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "adlb.xpt",
  "domain": "ADLB",
  "records": {
    "PARAMN": [1, 2],
    "AVAL": [10, 20]
  }
},
{
  "filename": "lookup.json",
  "domain": "LOOKUP",
  "records": {
    "PARAMN": ["1", "2"],
    "FLAG": ["Y", "N"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let keys = ["PARAMN".to_owned()];
    let joined = left_join_dataset_on(&datasets[0], &datasets[1], &keys, &keys, "LOOKUP.")
        .expect("left join");
    let flag = joined.frame().column("LOOKUP.FLAG").expect("joined flag");

    assert!(flag.get(0).expect("row 1").is_null());
    assert!(flag.get(1).expect("row 2").is_null());
}

#[test]
fn sort_dataset_by_columns_uses_numeric_order() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "adlb.xpt",
  "domain": "ADLB",
  "records": {
    "USUBJID": ["S10", "S2", "S1"],
    "AVAL": [10, 2, 1]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let dataset = load_dataset_package_json(&path)
        .expect("load package")
        .remove(0);
    let sorted = sort_dataset_by_columns(&dataset, &["AVAL".to_owned()], false).expect("sort rows");
    let subjects = sorted.frame().column("USUBJID").expect("subject");

    assert_eq!(subjects.get(0).expect("row 1").extract_str(), Some("S1"));
    assert_eq!(subjects.get(1).expect("row 2").extract_str(), Some("S2"));
    assert_eq!(subjects.get(2).expect("row 3").extract_str(), Some("S10"));
}

#[test]
fn sort_dataset_by_columns_keeps_tie_order_when_descending() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "adlb.xpt",
  "domain": "ADLB",
  "records": {
    "USUBJID": ["S1", "S2", "S3"],
    "AVAL": [1, 2, 2]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let dataset = load_dataset_package_json(&path)
        .expect("load package")
        .remove(0);
    let sorted = sort_dataset_by_columns(&dataset, &["AVAL".to_owned()], true).expect("sort rows");
    let subjects = sorted.frame().column("USUBJID").expect("subject");

    assert_eq!(subjects.get(0).expect("row 1").extract_str(), Some("S2"));
    assert_eq!(subjects.get(1).expect("row 2").extract_str(), Some("S3"));
    assert_eq!(subjects.get(2).expect("row 3").extract_str(), Some("S1"));
}

#[test]
fn derive_column_from_column_preserves_numeric_values() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "adlb.xpt",
  "domain": "ADLB",
  "records": {
    "AVAL": [1, 2]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let dataset = load_dataset_package_json(&path)
        .expect("load package")
        .remove(0);
    let derived = derive_column_from_column(&dataset, "AVAL_COPY", "AVAL").expect("derive column");
    let copy = derived.frame().column("AVAL_COPY").expect("copy");

    assert_eq!(copy.get(0).expect("row 1"), AnyValue::Int64(1));
    assert_eq!(copy.get(1).expect("row 2"), AnyValue::Int64(2));
}

#[test]
fn drop_dataset_columns_reports_all_columns_removed() {
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
    "DOMAIN": ["AE"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let dataset = load_dataset_package_json(&path)
        .expect("load package")
        .remove(0);
    let error = drop_dataset_columns(&dataset, &["DOMAIN".to_owned()])
        .expect_err("drop all columns fails clearly");

    assert!(
        matches!(error, DataError::InvalidDatasetPackage(message) if message.contains("cannot remove all columns"))
    );
}

#[test]
fn dataset_operations_filter_derive_group_count_and_sort_rows() {
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
    "USUBJID": ["S2", "S1", "S2"],
    "DOMAIN": ["AE", "AE", "AE"],
    "AESEQ": [2, 1, 3],
    "AESER": ["Y", "N", "Y"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let filtered =
        filter_dataset_by_mask(&datasets[0], &[true, false, true]).expect("filter dataset");
    assert_eq!(filtered.summary().row_count, 2);

    let derived = derive_literal_column(&filtered, "SOURCE", &Value::String("TEST".to_owned()))
        .expect("derive column");
    assert_eq!(
        derived
            .frame()
            .column("SOURCE")
            .expect("source column")
            .get(0)
            .expect("source row")
            .extract_str(),
        Some("TEST")
    );

    let counted = group_count_dataset(&derived, &["USUBJID".to_owned()], "USUBJID_COUNT")
        .expect("group count");
    assert_eq!(
        counted
            .frame()
            .column("USUBJID_COUNT")
            .expect("count column")
            .get(0)
            .expect("count row"),
        AnyValue::Int64(2)
    );

    let sorted = sort_dataset_by_columns(&counted, &["AESEQ".to_owned()], true).expect("sort rows");
    let numbered =
        row_number_dataset(&sorted, "ROWNUM", &["USUBJID".to_owned()]).expect("row number");
    assert_eq!(
        numbered
            .frame()
            .column("AESEQ")
            .expect("seq column")
            .get(0)
            .expect("first seq"),
        AnyValue::Int64(3)
    );
    assert_eq!(
        numbered
            .frame()
            .column("ROWNUM")
            .expect("row number column")
            .get(0)
            .expect("row number"),
        AnyValue::Int64(1)
    );
}

#[test]
fn distinct_values_without_keys_uses_all_rows_as_one_group() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("datasets.json");
    fs::write(
        &path,
        r#"{
  "datasets": [
{
  "filename": "dm.xpt",
  "domain": "DM",
  "records": {
    "USUBJID": ["S1", "S2", "S3"],
    "STUDYID": ["TEST-2", "TEST-1", "TEST-2"]
  }
}
  ]
}"#,
    )
    .expect("write package");

    let datasets = load_dataset_package_json(&path).expect("load package");
    let with_distinct = group_distinct_values_dataset(&datasets[0], &[], "STUDYID", "$dm_studyid")
        .expect("global distinct values");
    let distinct_values = with_distinct
        .frame()
        .column("$dm_studyid")
        .expect("distinct column");

    assert_eq!(
        distinct_values.get(0).expect("row 1").extract_str(),
        Some("TEST-1|TEST-2")
    );
    assert_eq!(
        distinct_values.get(2).expect("row 3").extract_str(),
        Some("TEST-1|TEST-2")
    );
}

#[derive(Debug, Clone)]
struct TestXptVariable {
    name: &'static str,
    label: &'static str,
    variable_type: XptVariableType,
    length: usize,
}

impl TestXptVariable {
    fn character(name: &'static str, length: usize, label: &'static str) -> Self {
        Self {
            name,
            label,
            variable_type: XptVariableType::Character,
            length,
        }
    }

    fn numeric(name: &'static str, label: &'static str) -> Self {
        Self::numeric_with_length(name, 8, label)
    }

    fn numeric_with_length(name: &'static str, length: usize, label: &'static str) -> Self {
        Self {
            name,
            label,
            variable_type: XptVariableType::Numeric,
            length,
        }
    }
}

#[derive(Debug, Clone)]
enum TestXptValue {
    Text(&'static str),
    Number(f64),
}

fn write_test_xpt(
    path: &std::path::Path,
    dataset_name: &str,
    variables: &[TestXptVariable],
    rows: &[Vec<TestXptValue>],
) {
    let mut bytes = Vec::new();
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******LIBRARY HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        "SAS     SAS     SASLIB  9.4     X64_10PRO                       18JUN26:00:00:00",
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******MEMBER  HEADER RECORD!!!!!!!000000000000000001600000000140",
    );
    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******DSCRPTR HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    push_xpt_card(
        &mut bytes,
        &format!(
            "SAS     {:<8}SASDATA 9.4     X64_10PRO                       18JUN26:00:00:00",
            dataset_name
        ),
    );
    push_xpt_card(&mut bytes, "18JUN26:00:00:00");
    push_xpt_card(
        &mut bytes,
        &format!(
            "HEADER RECORD*******NAMESTR HEADER RECORD!!!!!!!{:030}",
            variables.len()
        ),
    );

    let mut offset = 0_u32;
    let mut namestrs = Vec::new();
    for (index, variable) in variables.iter().enumerate() {
        let mut namestr = vec![0_u8; XPT_NAMESTR_LEN];
        let ntype = match variable.variable_type {
            XptVariableType::Numeric => 1_u16,
            XptVariableType::Character => 2_u16,
        };
        namestr[0..2].copy_from_slice(&ntype.to_be_bytes());
        namestr[4..6].copy_from_slice(&(variable.length as u16).to_be_bytes());
        namestr[6..8].copy_from_slice(&((index + 1) as u16).to_be_bytes());
        write_padded(&mut namestr[8..16], variable.name);
        write_padded(&mut namestr[16..56], variable.label);
        namestr[84..88].copy_from_slice(&offset.to_be_bytes());
        offset += variable.length as u32;
        namestrs.extend(namestr);
    }
    pad_to_xpt_card(&mut namestrs);
    bytes.extend(namestrs);

    push_xpt_card(
        &mut bytes,
        "HEADER RECORD*******OBS     HEADER RECORD!!!!!!!000000000000000000000000000000",
    );
    for row in rows {
        assert_eq!(row.len(), variables.len());
        for (variable, value) in variables.iter().zip(row) {
            match (&variable.variable_type, value) {
                (XptVariableType::Character, TestXptValue::Text(value)) => {
                    let start = bytes.len();
                    bytes.resize(start + variable.length, b' ');
                    write_padded(&mut bytes[start..start + variable.length], value);
                }
                (XptVariableType::Numeric, TestXptValue::Number(value)) => {
                    let encoded = f64_to_ibm_float(*value);
                    assert!(variable.length <= encoded.len());
                    bytes.extend(&encoded[..variable.length]);
                }
                _ => panic!("test XPT value type does not match variable type"),
            }
        }
    }
    pad_to_xpt_card(&mut bytes);

    fs::write(path, bytes).expect("write xpt");
}

fn push_xpt_card(bytes: &mut Vec<u8>, value: &str) {
    let start = bytes.len();
    bytes.resize(start + XPT_CARD_LEN, b' ');
    write_padded(&mut bytes[start..start + XPT_CARD_LEN], value);
}

fn write_padded(target: &mut [u8], value: &str) {
    let bytes = value.as_bytes();
    let len = bytes.len().min(target.len());
    target[..len].copy_from_slice(&bytes[..len]);
}

fn pad_to_xpt_card(bytes: &mut Vec<u8>) {
    let remainder = bytes.len() % XPT_CARD_LEN;
    if remainder != 0 {
        bytes.resize(bytes.len() + XPT_CARD_LEN - remainder, b' ');
    }
}

fn f64_to_ibm_float(value: f64) -> [u8; 8] {
    if value == 0.0 {
        return [0; 8];
    }

    let mut magnitude = value.abs();
    let mut exponent = 64_i32;
    while magnitude < 0.0625 {
        magnitude *= 16.0;
        exponent -= 1;
    }
    while magnitude >= 1.0 {
        magnitude /= 16.0;
        exponent += 1;
    }

    let mut output = [0_u8; 8];
    output[0] = (if value.is_sign_negative() { 0x80 } else { 0 })
        | (u8::try_from(exponent).expect("IBM exponent fits") & 0x7f);
    let fraction = (magnitude * 2_f64.powi(56)).round() as u64;
    for index in 0..7 {
        output[index + 1] = ((fraction >> (8 * (6 - index))) & 0xff) as u8;
    }
    output
}
