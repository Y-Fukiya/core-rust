use std::fs;

use core_engine::ExecutionStatus;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::{run_validation, DatasetLoader, ValidateRequest};

#[test]
fn run_validation_executes_usdm_reference_and_duplicate_jsonata_rules() {
    let dir = tempdir().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&rules_dir).expect("rules dir");
    fs::create_dir_all(&data_dir).expect("data dir");

    for (id, entity, output) in [
            (
                "CORE-000970",
                "StudyRole",
                "[\"name\", \"code\", \"appliesToIds\", \"StudyVersion.id\", \"StudyVersion.studyDesigns.id\"]",
            ),
            (
                "CORE-001022",
                "ProductOrganizationRole",
                "[\"name\", \"appliesToIds\", \"appliesTo name\"]",
            ),
            (
                "CORE-001024",
                "StudyDesign",
                "[\"name\", \"studyType\"]",
            ),
            (
                "CORE-001032",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001033",
                "StudyDesign",
                "[\"name\", \"characteristics\"]",
            ),
            (
                "CORE-001031",
                "StudyAmendmentReason",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"code\", \"primaryReason.code\"]",
            ),
            (
                "CORE-000999",
                "StudyDefinitionDocumentVersion",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"version\"]",
            ),
            (
                "CORE-000983",
                "Procedure",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"Activity.id\", \"Activity.name\", \"name\", \"studyInterventionId\", \"StudyIntervention.name\"]",
            ),
            (
                "CORE-000984",
                "SubjectEnrollment",
                "[\"StudyAmendment.id\", \"StudyAmendment.name\", \"name\", \"forGeographicScope\", \"forStudySiteId\", \"forStudyCohortId\"]",
            ),
            (
                "CORE-001010",
                "Substance",
                "[\"AdministrableProduct.id\", \"AdministrableProduct.name\", \"Ingredient.id\", \"Parent Substance.id\", \"Parent Substance.name\", \"name\", \"referenceSubstance.id\", \"referenceSubstance.name\"]",
            ),
            (
                "CORE-001018",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\"]",
            ),
            (
                "CORE-001019",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"category\", \"identifier\", \"Used in\"]",
            ),
            (
                "CORE-001025",
                "BiospecimenRetention",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"isRetained\"]",
            ),
            (
                "CORE-001027",
                "EligibilityCriterion",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"criterionItemId\"]",
            ),
            (
                "CORE-001028",
                "EligibilityCriterionItem",
                "[\"StudyVersion.id\", \"StudyVersion.versionIdentifier\", \"name\"]",
            ),
            (
                "CORE-001029",
                "StudyCohort",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.indications.id\", \"StudyDesignPopulation.id\", \"StudyDesignPopulation.name\", \"name\", \"Invalid indicationIds\"]",
            ),
            (
                "CORE-001030",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.studyInterventionIds\", \"name\", \"Invalid studyInterventionIds\", \"Invalid StudyIntervention.name\"]",
            ),
            (
                "CORE-001040",
                "StudyElement",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"name\", \"studyInterventionIds value\", \"Referenced intervention's parent StudyDesign.id\"]",
            ),
            (
                "CORE-001045",
                "StudyArm",
                "[\"StudyDesign.id\", \"StudyDesign.name\", \"StudyDesign.population.id\", \"StudyDesign.population.cohorts.id\", \"name\", \"populationId\"]",
            ),
            (
                "CORE-001042",
                "GeographicScope",
                "[\"type.code\", \"type.decode\", \"code.standardCode.code\", \"code.standardCode.decode\"]",
            ),
            (
                "CORE-001051",
                "NarrativeContent",
                "[\"StudyDefinitionDocument.id\", \"StudyDefinitionDocument.name\", \"StudyDefinitionDocumentVersion.id\", \"StudyDefinitionDocumentVersion.version\", \"name\", \"sectionNumber\", \"sectionTitle\"]",
            ),
            (
                "CORE-001050",
                "NarrativeContent",
                "[\"StudyProtocolDocument.id\", \"StudyProtocolDocument.name\", \"StudyProtocolDocumentVersion.id\", \"StudyProtocolDocumentVersion.protocolVersion\", \"name\", \"sectionNumber\", \"sectionTitle\", \"Invalid Reference\"]",
            ),
            (
                "CORE-001023",
                "InterventionalStudyDesign",
                "[\"name\", \"intentTypes\"]",
            ),
            (
                "CORE-001046",
                "StudyDesign",
                "[\"id\", \"name\", \"interventionModel.code\", \"interventionModel.decode\", \"# Study Interventions\"]",
            ),
            (
                "CORE-001013",
                "USDMObject",
                "[\"name\"]",
            ),
            (
                "CORE-001015",
                "USDMObject",
                "[\"name\"]",
            ),
        ] {
            fs::write(
                rules_dir.join(format!("{id}.json")),
                format!(
                    r#"{{
  "Core": {{ "Id": "{id}", "Status": "Published" }},
  "Rule Type": "JSONata",
  "Sensitivity": "Record",
  "Scope": {{ "Entities": {{ "Include": ["{entity}"] }} }},
  "Check": "$.**[instanceType=\"{entity}\"].{{\"check\": true}}",
  "Outcome": {{
    "Message": "USDM reference rule.",
    "Output Variables": {output}
  }}
}}"#
                ),
            )
            .expect("write rule");
        }

    fs::write(
            data_dir.join("usdm.json"),
            r#"{
  "study": {
    "documentedBy": [
      {
        "id": "StudyDefinitionDocument_1",
        "name": "Protocol",
        "versions": [
          {
            "id": "StudyDefinitionDocumentVersion_1",
            "version": "1",
            "instanceType": "StudyDefinitionDocumentVersion",
            "contents": [
              {
                "id": "NarrativeContent_1",
                "name": "Empty content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "1",
                "sectionTitle": "Overview"
              },
              {
                "id": "NarrativeContent_2",
                "name": "Invalid ref content",
                "instanceType": "NarrativeContent",
                "sectionNumber": "2",
                "sectionTitle": "Reference",
                "childIds": ["NarrativeContent_1"],
                "text": "<usdm:ref attribute=\"text\" id=\"MissingCriterion\" klass=\"EligibilityCriterion\"></usdm:ref>"
              }
            ]
          }
        ]
      }
    ],
    "versions": [
      {
        "id": "StudyVersion_1",
        "versionIdentifier": "1",
        "geographicScopes": [
          {
            "id": "GeographicScope_1",
            "name": "Global with code",
            "instanceType": "GeographicScope",
            "type": { "code": "C68846", "decode": "Global" },
            "code": { "standardCode": { "code": "US", "decode": "United States" } }
          }
        ],
        "duplicateObjects": [
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          },
          {
            "id": "DuplicateObject_1",
            "name": "Duplicate object name",
            "instanceType": "DuplicateObject"
          }
        ],
        "studyInterventions": [
          { "id": "StudyIntervention_1", "name": "Valid intervention" },
          { "id": "StudyIntervention_2", "name": "Other intervention" }
        ],
        "administrableProducts": [
          {
            "id": "AdmProd_1",
            "name": "Product",
            "ingredients": [
              {
                "id": "Ingredient_1",
                "substance": {
                  "id": "Substance_1",
                  "name": "Parent substance",
                  "referenceSubstance": {
                    "id": "Substance_2",
                    "name": "Reference substance",
                    "instanceType": "Substance",
                    "referenceSubstance": { "id": "Substance_3", "name": "Invalid nested reference" }
                  }
                }
              }
            ]
          }
        ],
        "studyDesigns": [
          {
            "id": "StudyDesign_1",
            "name": "Design",
            "instanceType": "ObservationalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "studyType": { "code": "C98388", "decode": "Interventional Study" },
            "characteristics": [
              { "id": "Code_1", "code": "C217006", "decode": "Single Country" },
              { "id": "Code_2", "code": "C217007", "decode": "Multiple Countries" },
              { "id": "Code_3", "code": "C46079", "decode": "Randomized" },
              { "id": "Code_4", "code": "C25689", "decode": "Stratification" }
            ],
            "activities": [
              {
                "id": "Activity_1",
                "name": "Activity",
                "definedProcedures": [
                  {
                    "id": "Procedure_1",
                    "name": "Procedure",
                    "instanceType": "Procedure",
                    "studyInterventionId": "StudyIntervention_2"
                  }
                ]
              }
            ],
            "population": {
              "id": "Population_1",
              "name": "Population",
              "criterionIds": ["EligibilityCriterion_1"],
              "cohorts": [
                {
                  "id": "Cohort_1",
                  "name": "Cohort",
                  "criterionIds": ["EligibilityCriterion_1"],
                  "indicationIds": ["Indication_bad"]
                }
              ]
            },
            "indications": [{ "id": "Indication_1", "name": "Indication" }],
            "eligibilityCriteria": [
              {
                "id": "EligibilityCriterion_1",
                "name": "Criterion 1",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "01"
              },
              {
                "id": "EligibilityCriterion_2",
                "name": "Criterion 2",
                "instanceType": "EligibilityCriterion",
                "criterionItemId": "EligibilityCriterionItem_1",
                "category": { "decode": "Inclusion Criteria" },
                "identifier": "02"
              }
            ],
            "biospecimenRetentions": [
              {
                "id": "BiospecimenRetention_1",
                "name": "Retention",
                "instanceType": "BiospecimenRetention",
                "isRetained": true
              }
            ],
            "elements": [
              {
                "id": "StudyElement_1",
                "name": "Element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_2"]
              }
            ],
            "arms": [
              {
                "id": "StudyArm_1",
                "name": "Arm",
                "instanceType": "StudyArm",
                "populationIds": ["Population_bad", "Population_worse"]
              }
            ]
          },
          {
            "id": "StudyDesign_2",
            "name": "Intent design",
            "instanceType": "InterventionalStudyDesign",
            "studyInterventionIds": ["StudyIntervention_1"],
            "interventionModel": { "code": "C82640", "decode": "Single Group Design" },
            "studyInterventions": [
              { "id": "StudyDesignIntervention_1", "name": "Embedded intervention 1" },
              { "id": "StudyDesignIntervention_2", "name": "Embedded intervention 2" }
            ],
            "elements": [
              {
                "id": "StudyElement_2",
                "name": "Cross-design element",
                "instanceType": "StudyElement",
                "studyInterventionIds": ["StudyIntervention_1"]
              }
            ],
            "intentTypes": [
              { "id": "IntentType_1", "code": "C123", "decode": "Intent" },
              { "id": "IntentType_2", "code": "C123", "decode": "Intent duplicate" },
              { "id": "IntentType_3", "code": "C456", "decode": "Other intent" },
              { "id": "IntentType_4", "code": "C456", "decode": "Other intent duplicate" }
            ]
          }
        ],
        "eligibilityCriterionItems": [
          {
            "id": "EligibilityCriterionItem_unused",
            "name": "Unused criterion item",
            "instanceType": "EligibilityCriterionItem"
          }
        ],
        "roles": [
          {
            "id": "Role_1",
            "name": "Invalid role scope",
            "instanceType": "StudyRole",
            "code": { "code": "C70793", "decode": "Sponsor" },
            "appliesToIds": ["StudyVersion_1", "StudyDesign_1"]
          }
        ],
        "productOrganizationRoles": [
          {
            "id": "ProductRole_1",
            "name": "Invalid product role",
            "instanceType": "ProductOrganizationRole",
            "appliesToIds": ["StudyVersion_1"]
          }
        ],
        "amendments": [
          {
            "id": "Amendment_1",
            "name": "Amendment",
            "enrollments": [
              {
                "id": "Enrollment_1",
                "name": "Enrollment",
                "instanceType": "SubjectEnrollment"
              }
            ],
            "primaryReason": {
              "id": "Reason_1",
              "instanceType": "StudyAmendmentReason",
              "code": { "code": "C17649", "decode": "Other" }
            },
            "secondaryReasons": [
              {
                "id": "Reason_2",
                "instanceType": "StudyAmendmentReason",
                "code": { "code": "C17649", "decode": "Other" }
              }
            ]
          }
        ]
      }
    ]
  }
}"#,
        )
        .expect("write json");

    for (id, dataset, expected_count) in [
        ("CORE-000970", "StudyRole", 1),
        ("CORE-001022", "ProductOrganizationRole", 1),
        ("CORE-001024", "StudyDesign", 1),
        ("CORE-001032", "StudyDesign", 1),
        ("CORE-001033", "StudyDesign", 1),
        ("CORE-001031", "StudyAmendmentReason", 1),
        ("CORE-000999", "StudyDefinitionDocumentVersion", 1),
        ("CORE-000983", "Procedure", 1),
        ("CORE-000984", "SubjectEnrollment", 1),
        ("CORE-001010", "Substance", 1),
        ("CORE-001018", "EligibilityCriterion", 1),
        ("CORE-001019", "EligibilityCriterion", 1),
        ("CORE-001025", "BiospecimenRetention", 1),
        ("CORE-001027", "EligibilityCriterion", 2),
        ("CORE-001028", "EligibilityCriterionItem", 1),
        ("CORE-001029", "StudyCohort", 1),
        ("CORE-001030", "StudyElement", 1),
        ("CORE-001040", "StudyElement", 2),
        ("CORE-001045", "StudyArm", 2),
        ("CORE-001042", "GeographicScope", 1),
        ("CORE-001051", "NarrativeContent", 1),
        ("CORE-001050", "NarrativeContent", 1),
        ("CORE-001023", "InterventionalStudyDesign", 2),
        ("CORE-001046", "StudyDesign", 1),
        ("CORE-001013", "USDMObject", 2),
        ("CORE-001015", "USDMObject", 2),
    ] {
        let outcome = run_validation(ValidateRequest {
            rule_paths: vec![rules_dir.join(format!("{id}.json"))],
            dataset_paths: vec![data_dir.clone()],
            dataset_loader: DatasetLoader::OpenRulesDataDir,
            ..Default::default()
        })
        .expect("run reference rule");
        assert_eq!(
            outcome.results[0].execution_status,
            ExecutionStatus::Failed,
            "{id}"
        );
        assert_eq!(outcome.results[0].error_count, expected_count, "{id}");
        assert_eq!(outcome.results[0].errors[0].dataset, dataset, "{id}");
    }
}
