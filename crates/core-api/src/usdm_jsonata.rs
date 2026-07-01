use core_data::LoadedDataset;
use core_engine::{RuleValidationResult, SkippedReason};
use core_rule_model::{Condition, ConditionGroup, ExecutableRule, Operator, ValueExpr};
use serde_json::Value;

pub(crate) fn apply_usdm_jsonata_semantics(rule: &mut ExecutableRule) {
    apply_usdm_planned_number_jsonata_semantics(rule);
    apply_usdm_study_role_jsonata_semantics(rule);
    apply_usdm_study_design_jsonata_semantics(rule);
    apply_usdm_study_version_jsonata_semantics(rule);
    apply_usdm_activity_jsonata_semantics(rule);
    apply_usdm_duration_jsonata_semantics(rule);
    apply_usdm_range_jsonata_semantics(rule);
    apply_usdm_person_name_jsonata_semantics(rule);
    apply_usdm_simple_recursive_jsonata_semantics(rule);
    apply_usdm_administrable_product_jsonata_semantics(rule);
    apply_usdm_administration_jsonata_semantics(rule);
    apply_usdm_strength_jsonata_semantics(rule);
    apply_usdm_reference_integrity_jsonata_semantics(rule);
    apply_usdm_planned_sex_jsonata_semantics(rule);
    apply_usdm_timeline_jsonata_semantics(rule);
    apply_usdm_scheduled_instance_jsonata_semantics(rule);
    apply_usdm_governance_date_jsonata_semantics(rule);
    apply_usdm_document_content_reference_jsonata_semantics(rule);
    apply_usdm_identifier_jsonata_semantics(rule);
    apply_usdm_object_jsonata_semantics(rule);
    apply_usdm_geographic_scope_jsonata_semantics(rule);
    apply_usdm_syntax_template_text_jsonata_semantics(rule);
    apply_usdm_narrative_content_jsonata_semantics(rule);
    apply_usdm_narrative_content_item_jsonata_semantics(rule);
    apply_usdm_abbreviation_jsonata_semantics(rule);
}

pub(crate) fn has_usdm_jsonata_semantics(rule: &ExecutableRule) -> bool {
    is_usdm_planned_number_jsonata_rule(rule)
        || is_usdm_study_role_jsonata_rule(rule)
        || is_usdm_study_design_jsonata_rule(rule)
        || is_usdm_study_version_jsonata_rule(rule)
        || is_usdm_activity_jsonata_rule(rule)
        || is_usdm_duration_jsonata_rule(rule)
        || is_usdm_range_jsonata_rule(rule)
        || is_usdm_person_name_jsonata_rule(rule)
        || is_usdm_simple_recursive_jsonata_rule(rule)
        || is_usdm_administrable_product_jsonata_rule(rule)
        || is_usdm_administration_jsonata_rule(rule)
        || is_usdm_strength_jsonata_rule(rule)
        || is_usdm_reference_integrity_jsonata_rule(rule)
        || is_usdm_planned_sex_jsonata_rule(rule)
        || is_usdm_timeline_jsonata_rule(rule)
        || is_usdm_scheduled_instance_jsonata_rule(rule)
        || is_usdm_governance_date_jsonata_rule(rule)
        || is_usdm_document_content_reference_jsonata_rule(rule)
        || is_usdm_identifier_jsonata_rule(rule)
        || is_usdm_object_jsonata_rule(rule)
        || is_usdm_geographic_scope_jsonata_rule(rule)
        || is_usdm_syntax_template_text_jsonata_rule(rule)
        || is_usdm_narrative_content_jsonata_rule(rule)
        || is_usdm_narrative_content_item_jsonata_rule(rule)
        || is_usdm_abbreviation_jsonata_rule(rule)
}

pub(crate) fn usdm_jsonata_execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Option<std::result::Result<Vec<LoadedDataset>, RuleValidationResult>> {
    if is_usdm_activity_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Activity"));
    }

    if is_usdm_duration_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Duration"));
    }

    if is_usdm_range_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Range"));
    }

    if is_usdm_person_name_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "PersonName"));
    }

    if is_usdm_administration_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Administration"));
    }

    if is_usdm_administrable_product_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "AdministrableProduct"));
    }

    if is_usdm_strength_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Strength"));
    }

    if matches!(rule.core_id.as_str(), "CORE-000971") {
        return Some(required_dataset(rule, datasets, "Address"));
    }

    if matches!(rule.core_id.as_str(), "CORE-001011" | "CORE-001031") {
        return Some(required_dataset(rule, datasets, "StudyAmendmentReason"));
    }

    if matches!(rule.core_id.as_str(), "CORE-001021" | "CORE-001022") {
        return Some(required_dataset(rule, datasets, "ProductOrganizationRole"));
    }

    if matches!(rule.core_id.as_str(), "CORE-001006") {
        return Some(required_dataset(rule, datasets, "BiomedicalConcept"));
    }

    if is_usdm_scheduled_instance_jsonata_rule(rule) {
        return Some(required_dataset(
            rule,
            datasets,
            "ScheduledActivityInstance",
        ));
    }

    if is_usdm_timeline_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "StudyDesign"));
    }

    if is_usdm_governance_date_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "GovernanceDate"));
    }

    if is_usdm_document_content_reference_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "DocumentContentReference"));
    }

    if is_usdm_object_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "USDMObject"));
    }

    if is_usdm_geographic_scope_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "GeographicScope"));
    }

    if is_usdm_syntax_template_text_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "SyntaxTemplateText"));
    }

    if is_usdm_narrative_content_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "NarrativeContent"));
    }

    if is_usdm_narrative_content_item_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "NarrativeContentItem"));
    }

    if is_usdm_abbreviation_jsonata_rule(rule) {
        return Some(required_dataset(rule, datasets, "Abbreviation"));
    }

    if rule.core_id == "CORE-001070" {
        return Some(required_dataset(rule, datasets, "StudyRoleBlinding"));
    }

    if rule.core_id == "CORE-001077" {
        return Some(required_dataset(
            rule,
            datasets,
            "InterventionalStudyDesign",
        ));
    }

    if matches!(
        rule.core_id.as_str(),
        "CORE-000998"
            | "CORE-000961"
            | "CORE-001004"
            | "CORE-001005"
            | "CORE-001017"
            | "CORE-001036"
            | "CORE-001048"
            | "CORE-001065"
    ) {
        return Some(required_dataset(rule, datasets, "StudyDesign"));
    }

    if matches!(rule.core_id.as_str(), "CORE-000980") {
        return Some(dataset_with_fallback(
            rule,
            datasets,
            "StudyDesignCharacteristicDuplicate",
            "StudyDesign",
        ));
    }

    if matches!(rule.core_id.as_str(), "CORE-001002") {
        return Some(dataset_with_fallback(
            rule,
            datasets,
            "StudyDesignSubTypeDuplicate",
            "StudyDesign",
        ));
    }

    if matches!(rule.core_id.as_str(), "CORE-001003") {
        return Some(dataset_with_fallback(
            rule,
            datasets,
            "StudyDesignTherapeuticAreaDuplicate",
            "StudyDesign",
        ));
    }

    None
}

fn required_dataset(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    name: &str,
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    let Some(dataset) = find_dataset(datasets, name) else {
        return Err(RuleValidationResult::skipped_rule(
            rule.core_id.clone(),
            SkippedReason::EvaluationError,
            format!("Rule {} requires {name} dataset", rule.core_id),
        ));
    };
    Ok(vec![dataset.clone()])
}

fn dataset_with_fallback(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    preferred: &str,
    fallback: &str,
) -> std::result::Result<Vec<LoadedDataset>, RuleValidationResult> {
    if let Some(dataset) = find_dataset(datasets, preferred) {
        return Ok(vec![dataset.clone()]);
    }
    required_dataset(rule, datasets, fallback)
}

fn find_dataset<'a>(datasets: &'a [LoadedDataset], name: &str) -> Option<&'a LoadedDataset> {
    datasets.iter().find(|dataset| {
        dataset.metadata.name.eq_ignore_ascii_case(name)
            || dataset
                .metadata
                .domain
                .as_deref()
                .is_some_and(|domain| domain.eq_ignore_ascii_case(name))
    })
}

fn apply_usdm_planned_number_jsonata_semantics(rule: &mut ExecutableRule) {
    if let Some(quantity) = usdm_planned_number_unit_quantity_name(rule) {
        rule.conditions = ConditionGroup::Any(vec![
            bool_condition(format!("{quantity}.has_unit"), true),
            bool_condition(format!("cohorts.{quantity}.has_unit"), true),
        ]);
        return;
    }

    let Some(quantity) = usdm_planned_number_consistency_quantity_name(rule) else {
        return;
    };

    let incomplete_cohort_condition = if quantity == "plannedEnrollmentNumber" {
        bool_condition(format!("cohorts.{quantity}.any_missing"), true)
    } else {
        bool_condition(format!("cohorts.{quantity}.all_present"), false)
    };

    rule.conditions = ConditionGroup::Any(vec![
        ConditionGroup::All(vec![
            bool_condition(format!("{quantity}.present"), true),
            bool_condition(format!("cohorts.{quantity}.any_present"), true),
        ]),
        ConditionGroup::All(vec![
            bool_condition(format!("{quantity}.present"), false),
            bool_condition(format!("cohorts.{quantity}.any_present"), true),
            incomplete_cohort_condition,
        ]),
    ]);
}

fn is_usdm_planned_number_jsonata_rule(rule: &ExecutableRule) -> bool {
    usdm_planned_number_unit_quantity_name(rule).is_some()
        || usdm_planned_number_consistency_quantity_name(rule).is_some()
}

fn apply_usdm_study_role_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000974" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("code.code", "C70793"),
                bool_condition("sponsor_role_applies_to_study_version".to_owned(), false),
            ]);
        }
        "CORE-000997" => {
            rule.conditions =
                bool_condition("study_role_has_assigned_persons_and_orgs".to_owned(), true);
        }
        "CORE-001000" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("code.code", "C70793"),
                bool_condition("sponsor_role_has_exactly_one_valid_org".to_owned(), false),
            ]);
        }
        "CORE-000970" => {
            rule.conditions =
                bool_condition("study_role_invalid_applies_to_scope".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_study_role_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000970" | "CORE-000974" | "CORE-000997" | "CORE-001000"
    )
}

fn apply_usdm_study_design_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000948" => {
            rule.conditions = bool_condition("study_cell_arm_epoch_duplicate".to_owned(), true);
        }
        "CORE-000998" => {
            rule.conditions = bool_condition(
                "study_design_duplicate_document_version_ids".to_owned(),
                true,
            );
        }
        "CORE-000980" | "CORE-001002" | "CORE-001003" => {
            rule.conditions = bool_condition("study_design_duplicate_list_row".to_owned(), true);
        }
        "CORE-001004" => {
            rule.conditions = bool_condition("observational_design_wrong_class".to_owned(), true);
        }
        "CORE-001005" => {
            rule.conditions = bool_condition("observational_design_wrong_phase".to_owned(), true);
        }
        "CORE-001017" => {
            rule.conditions =
                bool_condition("study_design_single_and_multi_centre".to_owned(), true);
        }
        "CORE-001024" => {
            rule.conditions = bool_condition("interventional_design_wrong_class".to_owned(), true);
        }
        "CORE-001032" => {
            rule.conditions = bool_condition(
                "study_design_single_and_multiple_countries".to_owned(),
                true,
            );
        }
        "CORE-001033" => {
            rule.conditions = bool_condition(
                "study_design_randomization_characteristic_conflict".to_owned(),
                true,
            );
        }
        "CORE-001023" => {
            rule.conditions =
                bool_condition("study_design_duplicate_intent_types".to_owned(), true);
        }
        "CORE-001046" => {
            rule.conditions = bool_condition(
                "study_design_intervention_model_count_inconsistent".to_owned(),
                true,
            );
        }
        "CORE-000961" => {
            rule.conditions = bool_condition(
                "study_design_encounter_timeline_order_mismatch".to_owned(),
                true,
            );
        }
        "CORE-001048" => {
            rule.conditions = bool_condition(
                "study_design_epoch_timeline_order_mismatch".to_owned(),
                true,
            );
        }
        "CORE-000999" => {
            rule.conditions = bool_condition(
                "study_definition_document_version_unreferenced".to_owned(),
                true,
            );
        }
        "CORE-001036" => {
            rule.conditions = number_condition("# Primary endpoints", Operator::EqualTo, 0);
        }
        "CORE-001038" => {
            rule.conditions = bool_condition("condition_applies_to_invalid".to_owned(), true);
        }
        "CORE-001049" => {
            rule.conditions = bool_condition("parameter_map_reference_invalid".to_owned(), true);
        }
        "CORE-001065" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("studyType.code", "C98388"),
                number_condition("# Referenced Study Interventions", Operator::LessThan, 1),
            ]);
        }
        "CORE-001077" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("studyType.code", "C98388"),
                ConditionGroup::Any(vec![
                    string_condition("model.code", "C82637"),
                    string_condition("model.code", "C82639"),
                    string_condition("model.code", "C82638"),
                ]),
                number_condition(
                    "# Referenced Study Interventions",
                    Operator::LessThanOrEqualTo,
                    1,
                ),
            ]);
        }
        "CORE-001072" => {
            rule.conditions =
                bool_condition("blinding_schema_missing_masked_role".to_owned(), true);
        }
        "CORE-001071" => {
            rule.conditions = ConditionGroup::All(vec![
                string_condition("blindingSchema.code", "C15228"),
                number_condition("# Masked Roles", Operator::LessThan, 2),
            ]);
        }
        "CORE-001070" => {
            rule.conditions =
                bool_condition("study_role_masked_for_open_label_design".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_study_design_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000948"
            | "CORE-000980"
            | "CORE-000998"
            | "CORE-001002"
            | "CORE-001003"
            | "CORE-001004"
            | "CORE-001005"
            | "CORE-001017"
            | "CORE-001024"
            | "CORE-001023"
            | "CORE-001046"
            | "CORE-000961"
            | "CORE-001048"
            | "CORE-001032"
            | "CORE-001033"
            | "CORE-000999"
            | "CORE-001036"
            | "CORE-001038"
            | "CORE-001049"
            | "CORE-001065"
            | "CORE-001070"
            | "CORE-001071"
            | "CORE-001072"
            | "CORE-001077"
    )
}

fn apply_usdm_object_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001075" => {
            rule.conditions = bool_condition("usdm_id_contains_space".to_owned(), true);
        }
        "CORE-001013" => {
            rule.conditions = bool_condition("usdm_duplicate_name_for_class".to_owned(), true);
        }
        "CORE-001015" => {
            rule.conditions = bool_condition("usdm_duplicate_id".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_object_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001075" | "CORE-001013" | "CORE-001015"
    )
}

fn apply_usdm_geographic_scope_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001042" {
        rule.conditions = bool_condition("geographic_scope_global_code_mismatch".to_owned(), true);
    }
}

fn is_usdm_geographic_scope_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001042"
}

fn apply_usdm_syntax_template_text_jsonata_semantics(rule: &mut ExecutableRule) {
    if matches!(rule.core_id.as_str(), "CORE-001037" | "CORE-001074") {
        rule.conditions = bool_condition("syntax_template_tag_invalid".to_owned(), true);
    }
}

fn is_usdm_syntax_template_text_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001037" | "CORE-001074")
}

fn apply_usdm_narrative_content_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000944" => {
            rule.conditions = bool_condition("narrative_content_item_id_invalid".to_owned(), true);
        }
        "CORE-000964" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_number_missing".to_owned(),
                true,
            );
        }
        "CORE-000965" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_title_missing".to_owned(),
                true,
            );
        }
        "CORE-001055" => {
            rule.conditions = bool_condition("narrative_content_peer_ref_invalid".to_owned(), true);
        }
        "CORE-001051" => {
            rule.conditions = bool_condition("narrative_content_missing_link".to_owned(), true);
        }
        "CORE-001050" => {
            rule.conditions = bool_condition("narrative_content_invalid_usdm_ref".to_owned(), true);
        }
        "CORE-001041" => {
            rule.conditions = bool_condition(
                "narrative_content_display_section_number_duplicate".to_owned(),
                true,
            );
        }
        _ => {}
    }
}

fn is_usdm_narrative_content_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000944"
            | "CORE-000964"
            | "CORE-000965"
            | "CORE-001041"
            | "CORE-001050"
            | "CORE-001051"
            | "CORE-001055"
    )
}

fn apply_usdm_narrative_content_item_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001073" {
        rule.conditions = bool_condition("narrative_content_ref_invalid".to_owned(), true);
    }
}

fn is_usdm_narrative_content_item_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001073"
}

fn apply_usdm_abbreviation_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001067" => {
            rule.conditions =
                bool_condition("abbreviation_expanded_text_duplicate".to_owned(), true);
        }
        "CORE-001053" => {
            rule.conditions = bool_condition("abbreviation_text_duplicate".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_abbreviation_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001067" | "CORE-001053")
}

fn apply_usdm_study_version_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001052" => {
            rule.conditions = bool_condition("duplicate_document_version_ids".to_owned(), true);
        }
        "CORE-001054" => {
            rule.conditions = number_condition("# Sponsor Identifiers", Operator::NotEqualTo, 1);
        }
        "CORE-000973" => {
            rule.conditions = number_condition("# Sponsor Roles", Operator::NotEqualTo, 1);
        }
        _ => {}
    }
}

fn is_usdm_study_version_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001052" | "CORE-001054" | "CORE-000973"
    )
}

fn apply_usdm_activity_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000954" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_children_with_details".to_owned(), true),
            ]);
        }
        "CORE-001062" => {
            rule.conditions = bool_condition("activity_child_id_invalid".to_owned(), true);
        }
        "CORE-001066" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_child_order_invalid".to_owned(), true),
            ]);
        }
        "CORE-001047" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("activity_summary_row".to_owned(), true),
                bool_condition("activity_bc_category_overlap".to_owned(), true),
            ]);
        }
        _ => {}
    }
}

fn is_usdm_activity_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000954" | "CORE-001047" | "CORE-001062" | "CORE-001066"
    )
}

fn apply_usdm_duration_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000994" => {
            rule.conditions = bool_condition("duration_missing_text_and_quantity".to_owned(), true);
        }
        "CORE-000995" => {
            rule.conditions = bool_condition("duration_vary_quantity_conflict".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_duration_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000994" | "CORE-000995")
}

fn apply_usdm_range_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001009" => {
            rule.conditions = bool_condition("range_min_not_less_than_max".to_owned(), true);
        }
        "CORE-001012" => {
            rule.conditions = bool_condition("range_unit_xor".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_range_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-001009" | "CORE-001012")
}

fn apply_usdm_person_name_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001014" {
        rule.conditions =
            bool_condition("person_name_missing_text_and_family_name".to_owned(), true);
    }
}

fn is_usdm_person_name_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001014"
}

fn apply_usdm_simple_recursive_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000971" => {
            rule.conditions = bool_condition("address_all_blank".to_owned(), true);
        }
        "CORE-001011" => {
            rule.conditions = bool_condition("primary_reason_not_applicable".to_owned(), true);
        }
        "CORE-001021" => {
            rule.conditions = bool_condition("product_role_missing_valid_target".to_owned(), true);
        }
        "CORE-001022" => {
            rule.conditions = bool_condition("product_role_invalid_target".to_owned(), true);
        }
        "CORE-001006" => {
            rule.conditions =
                bool_condition("biomedical_concept_synonym_equals_label".to_owned(), true);
        }
        "CORE-001031" => {
            rule.conditions = bool_condition("secondary_reason_matches_primary".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_simple_recursive_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000971"
            | "CORE-001006"
            | "CORE-001011"
            | "CORE-001021"
            | "CORE-001022"
            | "CORE-001031"
    )
}

fn apply_usdm_administrable_product_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-001001" {
        rule.conditions = bool_condition(
            "administrable_product_embedded_only_sourcing".to_owned(),
            true,
        );
    }
}

fn is_usdm_administrable_product_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-001001"
}

fn apply_usdm_administration_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000966" => {
            rule.conditions = bool_condition("administration_dose_route_xor".to_owned(), true);
        }
        "CORE-000967" => {
            rule.conditions =
                bool_condition("administration_dose_without_frequency".to_owned(), true);
        }
        "CORE-000969" => {
            rule.conditions = bool_condition("administration_dose_product_xor".to_owned(), true);
        }
        "CORE-000986" => {
            rule.conditions =
                bool_condition("administration_duplicate_embedded_product".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_administration_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000966" | "CORE-000967" | "CORE-000969" | "CORE-000986"
    )
}

fn apply_usdm_strength_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-001007" => {
            rule.conditions =
                bool_condition("strength_numerator_value_missing_unit".to_owned(), true);
        }
        "CORE-001008" => {
            rule.conditions =
                bool_condition("strength_numerator_range_missing_unit".to_owned(), true);
        }
        "CORE-001020" => {
            rule.conditions = bool_condition("strength_denominator_missing_unit".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_strength_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-001007" | "CORE-001008" | "CORE-001020"
    )
}

fn apply_usdm_reference_integrity_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000983" => {
            rule.conditions =
                bool_condition("procedure_invalid_study_intervention".to_owned(), true);
        }
        "CORE-000984" => {
            rule.conditions = bool_condition("subject_enrollment_invalid_scope".to_owned(), true);
        }
        "CORE-001010" => {
            rule.conditions = bool_condition("substance_reference_has_reference".to_owned(), true);
        }
        "CORE-001018" => {
            rule.conditions = bool_condition("eligibility_criterion_unused".to_owned(), true);
        }
        "CORE-001019" => {
            rule.conditions = bool_condition(
                "eligibility_criterion_used_in_population_and_cohort".to_owned(),
                true,
            );
        }
        "CORE-001025" => {
            rule.conditions =
                bool_condition("biospecimen_retained_missing_includes_dna".to_owned(), true);
        }
        "CORE-001026" => {
            rule.conditions = bool_condition("study_arm_missing_epoch_refs".to_owned(), true);
        }
        "CORE-001027" => {
            rule.conditions =
                bool_condition("eligibility_criterion_duplicate_item".to_owned(), true);
        }
        "CORE-001028" => {
            rule.conditions = bool_condition("eligibility_criterion_item_unused".to_owned(), true);
        }
        "CORE-001029" => {
            rule.conditions = bool_condition("study_cohort_invalid_indication".to_owned(), true);
        }
        "CORE-001030" => {
            rule.conditions =
                bool_condition("study_element_invalid_study_intervention".to_owned(), true);
        }
        "CORE-001040" => {
            rule.conditions = bool_condition(
                "study_element_cross_design_study_intervention".to_owned(),
                true,
            );
        }
        "CORE-001045" => {
            rule.conditions = bool_condition("study_arm_invalid_population".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_reference_integrity_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000983"
            | "CORE-000984"
            | "CORE-001010"
            | "CORE-001018"
            | "CORE-001019"
            | "CORE-001025"
            | "CORE-001026"
            | "CORE-001027"
            | "CORE-001028"
            | "CORE-001029"
            | "CORE-001030"
            | "CORE-001040"
            | "CORE-001045"
    )
}

fn apply_usdm_planned_sex_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id != "CORE-000996" {
        return;
    }

    rule.conditions = bool_condition("plannedSex.invalid".to_owned(), true);
}

fn is_usdm_planned_sex_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000996"
}

fn apply_usdm_timeline_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000407" => {
            rule.conditions = number_condition("# Main timelines", Operator::NotEqualTo, 1);
        }
        "CORE-001016" => {
            rule.conditions = ConditionGroup::All(vec![
                bool_condition("mainTimeline".to_owned(), true),
                bool_condition("plannedDuration.present".to_owned(), false),
            ]);
        }
        _ => {}
    }
}

fn is_usdm_timeline_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000407" | "CORE-001016")
}

fn apply_usdm_scheduled_instance_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000950" => {
            rule.conditions =
                bool_condition("scheduled_instance_epoch_wrong_design".to_owned(), true);
        }
        "CORE-001039" => {
            rule.conditions =
                bool_condition("scheduled_instance_encounter_wrong_design".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_scheduled_instance_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000950" | "CORE-001039")
}

fn apply_usdm_governance_date_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-000968" {
        rule.conditions = bool_condition("governance_date_global_type_duplicate".to_owned(), true);
    }
}

fn is_usdm_governance_date_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000968"
}

fn apply_usdm_document_content_reference_jsonata_semantics(rule: &mut ExecutableRule) {
    if rule.core_id == "CORE-000985" {
        rule.conditions = bool_condition(
            "document_content_reference_section_one_to_one_invalid".to_owned(),
            true,
        );
    }
}

fn is_usdm_document_content_reference_jsonata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == "CORE-000985"
}

fn apply_usdm_identifier_jsonata_semantics(rule: &mut ExecutableRule) {
    match rule.core_id.as_str() {
        "CORE-000955" => {
            rule.conditions = bool_condition("identifier_text_scope_duplicate".to_owned(), true);
        }
        "CORE-000956" => {
            rule.conditions = bool_condition("study_identifier_scope_duplicate".to_owned(), true);
        }
        _ => {}
    }
}

fn is_usdm_identifier_jsonata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), "CORE-000955" | "CORE-000956")
}

fn usdm_planned_number_unit_quantity_name(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        "CORE-000981" => Some("plannedEnrollmentNumber"),
        "CORE-000982" => Some("plannedCompletionNumber"),
        _ => None,
    }
}

fn usdm_planned_number_consistency_quantity_name(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        "CORE-000963" => Some("plannedEnrollmentNumber"),
        "CORE-000962" => Some("plannedCompletionNumber"),
        _ => None,
    }
}

fn bool_condition(target: String, value: bool) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::Bool(value)),
        options: Default::default(),
    })
}

fn string_condition(target: &str, value: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::String(value.to_owned())),
        options: Default::default(),
    })
}

fn number_condition(target: &str, operator: Operator, value: i64) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target.to_owned()),
        operator,
        comparator: ValueExpr::Literal(Value::Number(serde_json::Number::from(value))),
        options: Default::default(),
    })
}
