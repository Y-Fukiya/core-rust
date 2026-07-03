use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, ValidateRequest};

#[test]
fn run_validation_executes_core_000690_label_uniqueness_direction() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000690.json"),
        r#"{
  "Core": { "Id": "CORE-000690", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Record",
  "Rule Type": "Variable Metadata Check",
  "Check": { "name": "variable_name", "operator": "is_not_unique_relationship", "value": "variable_label" },
  "Outcome": {
    "Message": "Variable label is not unique for each variable in the dataset.",
    "Output Variables": ["variable_label", "variable_name"]
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
      "filename": "gt.xpt",
      "domain": "GT",
      "variables": [
        { "name": "GTREFID", "label": "Reference ID", "type": "Char", "length": 8 },
        { "name": "GTREFID", "label": "Lab Test or Examination Short Name", "type": "Char", "length": 50 }
      ],
      "records": { "GTREFID": ["A"] }
    },
    {
      "filename": "relref.xpt",
      "domain": "RELREF",
      "variables": [
        { "name": "LEVEL", "label": "Reference ID Generation Level", "type": "Num", "length": 8 },
        { "name": "LVLDESC", "label": "Reference ID Generation Level", "type": "Char", "length": 50 }
      ],
      "records": { "LEVEL": [1], "LVLDESC": ["Level 1"] }
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

    let failed = outcome
        .results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert_eq!(failed[0].dataset, "RELREF");
    assert_eq!(failed[0].error_count, 2);
    assert_eq!(
        failed[0]
            .errors
            .iter()
            .filter_map(|error| error.row)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn run_validation_executes_domain_label_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-DOMAIN-LABEL.json"),
        r#"{
  "Core": { "Id": "CORE-DOMAIN-LABEL", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "Category must not repeat the domain label" }
}"#,
    )
    .expect("write domain label rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1", "S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2", "SUBJ3"],
        "LBCAT": ["Laboratory Test Results", "LB", "CHEMISTRY"]
      }
    }
  ]
}"#,
    )
    .expect("write domain label data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 1);
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000272_domain_label_cat_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000272.json"),
        r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SDTMIG", "Version": "3.4" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
    )
    .expect("write domain label rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "LBCAT": ["Laboratory Test Results"]
      }
    }
  ]
}"#,
    )
    .expect("write domain label oracle gap data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000272".to_owned()],
        standard: Some("SDTMIG".to_owned()),
        standard_version: Some("3.4".to_owned()),
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
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
}

#[test]
fn run_validation_executes_core_000272_sendig_domain_name_cat_rule() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-000272.json"),
        r#"{
  "Core": { "Id": "CORE-000272", "Status": "Published" },
  "Authorities": [
    { "Standards": [{ "Name": "SENDIG", "Version": "3.1" }] }
  ],
  "Scope": { "Domains": { "Include": ["LB"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$domain_label",
      "operator": "domain_label"
    }
  ],
  "Check": {
    "name": "--CAT",
    "operator": "equal_to_case_insensitive",
    "value": "$domain_label"
  },
  "Outcome": { "Message": "--CAT is equal to DOMAIN." }
}"#,
    )
    .expect("write SENDIG domain name rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "lb.xpt",
      "domain": "LB",
      "label": "Laboratory Test Results",
      "records": {
        "STUDYID": ["S1"],
        "DOMAIN": ["LB"],
        "USUBJID": ["SUBJ1"],
        "LBSEQ": [1],
        "LBCAT": ["LB"]
      }
    }
  ]
}"#,
    )
    .expect("write SENDIG domain name data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        include_rules: vec!["CORE-000272".to_owned()],
        standard: Some("SENDIG".to_owned()),
        standard_version: Some("3.1".to_owned()),
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
    assert_eq!(outcome.results[0].errors[0].row, Some(1));
    assert_eq!(
        outcome.results[0].errors[0].variables,
        vec!["LBCAT".to_owned(), "DOMAIN".to_owned()]
    );
}

#[test]
fn run_validation_executes_extract_metadata_dataset_name_string_part_check() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-EXTRACT-METADATA.json"),
        r#"{
  "Core": { "Id": "CORE-EXTRACT-METADATA", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["SUPP--"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$dataset_name",
      "name": "dataset_name",
      "operator": "extract_metadata"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "does_not_equal_string_part",
    "regex": ".{4}(..).*",
    "value": "$dataset_name"
  },
  "Outcome": { "Message": "RDOMAIN must match the parent domain in the SUPP dataset name" }
}"#,
    )
    .expect("write extract metadata rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "suppae.xpt",
      "domain": "SUPPAE",
      "records": {
        "STUDYID": ["S1", "S1"],
        "RDOMAIN": ["AE", "XX"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "QNAM": ["AETERM", "BAD"]
      }
    }
  ]
}"#,
    )
    .expect("write extract metadata data");

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
fn run_validation_executes_get_xhtml_errors_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-XHTML.json"),
        r#"{
  "Core": { "Id": "CORE-XHTML", "Status": "Published" },
  "Scope": { "Entities": { "Include": ["EligibilityCriterion"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$xhtml_errors",
      "name": "text",
      "namespace": "http://www.cdisc.org/ns/usdm/xhtml/v1.0",
      "operator": "get_xhtml_errors"
    }
  ],
  "Check": {
    "all": [
      { "name": "rel_type", "operator": "equal_to", "value": "definition" },
      { "name": "$xhtml_errors", "operator": "non_empty" }
    ]
  },
  "Outcome": { "Message": "The text attribute contains non-conformant XHTML." }
}"#,
    )
    .expect("write xhtml rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "EligibilityCriterion.csv",
      "domain": "EligibilityCriterion",
      "records": {
        "rel_type": ["definition", "definition", "definition", "definition", "definition", "label"],
        "name": ["VALID", "BAD_TAG", "BAD_XML", "BAD_LIST", "BAD_REF", "IGNORED"],
        "text": [
          "<p>At least <usdm:tag name=\"min_age\"/> years.</p>",
          "<p><usdm:tag nam=\"min_age\"/></p>",
          "Insulin-dependent & diabetic",
          "<div><ul><li><p>Allowed item</p></li><ul></ul></ul></div>",
          "<p><usdm:ref attribute=\"text\" klass=\"StudyTitle\"/></p>",
          "Insulin-dependent & diabetic"
        ]
      }
    }
  ]
}"#,
    )
    .expect("write xhtml data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(outcome.results[0].error_count, 4);
    assert_eq!(outcome.results[0].errors[0].row, Some(2));
    assert_eq!(outcome.results[0].errors[1].row, Some(3));
    assert_eq!(outcome.results[0].errors[2].row, Some(4));
    assert_eq!(outcome.results[0].errors[3].row, Some(5));
}

#[test]
fn run_validation_executes_study_domains_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-STUDY-DOMAINS.json"),
        r#"{
  "Core": { "Id": "CORE-STUDY-DOMAINS", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["RELREC"] } },
  "Sensitivity": "Record",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$study_domains",
      "operator": "study_domains"
    }
  ],
  "Check": {
    "name": "RDOMAIN",
    "operator": "is_not_contained_by",
    "value": "$study_domains"
  },
  "Outcome": {
    "Message": "RDOMAIN does not represent a dataset present in the study",
    "Output Variables": ["RDOMAIN"]
  }
}"#,
    )
    .expect("write study domains rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "relrec.xpt",
      "domain": "RELREC",
      "records": {
        "STUDYID": ["S1", "S1"],
        "USUBJID": ["SUBJ1", "SUBJ2"],
        "RELID": ["R1", "R2"],
        "RDOMAIN": ["AE", "XX"]
      }
    },
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "records": {
        "STUDYID": ["S1"],
        "USUBJID": ["SUBJ1"],
        "AESEQ": [1]
      }
    }
  ]
}"#,
    )
    .expect("write study domains data");

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
        vec!["RDOMAIN".to_owned()]
    );
}

#[test]
fn run_validation_executes_variable_count_operation() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    fs::write(
        rules_dir.join("CORE-VARIABLE-COUNT.json"),
        r#"{
  "Core": { "Id": "CORE-VARIABLE-COUNT", "Status": "Published" },
  "Scope": { "Domains": { "Include": ["ALL"] }, "Classes": { "Include": ["ALL"] } },
  "Sensitivity": "Dataset",
  "Rule Type": "Record Data",
  "Operations": [
    {
      "id": "$VARIABLE_COUNT",
      "name": "--LNKGRP",
      "operator": "variable_count"
    }
  ],
  "Check": {
    "all": [
      { "name": "--LNKGRP", "operator": "exists" },
      { "name": "$VARIABLE_COUNT", "operator": "less_than", "value": 2 }
    ]
  },
  "Outcome": {
    "Message": "LNKGRP variable is not found in any of the other domains.",
    "Output Variables": ["--LNKGRP", "$VARIABLE_COUNT"]
  }
}"#,
    )
    .expect("write variable count rule");

    let dataset_path = data_dir.join("datasets.json");
    fs::write(
        &dataset_path,
        r#"{
  "datasets": [
    {
      "filename": "ae.xpt",
      "domain": "AE",
      "variables": [
        { "name": "STUDYID" },
        { "name": "AESEQ" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "AESEQ": [1]
      }
    },
    {
      "filename": "fa.xpt",
      "domain": "FA",
      "variables": [
        { "name": "STUDYID" },
        { "name": "FASEQ" },
        { "name": "FALNKGRP" }
      ],
      "records": {
        "STUDYID": ["S1"],
        "FASEQ": [1],
        "FALNKGRP": ["CDISC001 - 1"]
      }
    }
  ]
}"#,
    )
    .expect("write variable count data");

    let outcome = run_validation(ValidateRequest {
        rule_paths: vec![rules_dir],
        dataset_paths: vec![dataset_path],
        ..Default::default()
    })
    .expect("run validation");

    let result = outcome
        .results
        .iter()
        .find(|result| result.dataset == "FA")
        .expect("FA result");
    assert_eq!(result.execution_status, ExecutionStatus::Failed);
    assert_eq!(result.error_count, 1);
    assert_eq!(result.errors[0].row, None);
    assert_eq!(
        result.errors[0].variables,
        vec!["FALNKGRP".to_owned(), "$VARIABLE_COUNT".to_owned()]
    );
}
