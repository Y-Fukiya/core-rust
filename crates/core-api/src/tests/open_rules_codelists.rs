use super::{
    static_codelist, static_codelist_matches_version, static_codelist_term_by_code,
    static_codelist_term_by_value, static_codelist_term_matches_version, valid_codelist_dates,
    valid_codelist_dates_for_operation,
};
use core_rule_model::OperationSpec;

#[test]
fn static_codelist_resolves_ddf_organization_type_terms() {
    assert!(valid_codelist_dates().contains(&"2025-09-26"));

    let codelist = static_codelist("C188724").expect("organization type codelist");
    assert!(codelist.extensible);

    let sponsor_2024 =
        static_codelist_term_by_code("C188724", &codelist, "C70793", Some("2024-09-27"))
            .expect("2024 clinical study sponsor");
    assert_eq!(sponsor_2024.value, "Clinical Study Sponsor");
    assert_eq!(sponsor_2024.pref_term, "Clinical Study Sponsor");

    let sponsor_2025 =
        static_codelist_term_by_code("C188724", &codelist, "C70793", Some("2025-09-26"))
            .expect("2025 clinical study sponsor");
    assert_eq!(sponsor_2025.value, "Study Sponsor");
    assert_eq!(sponsor_2025.pref_term, "Clinical Study Sponsor");

    let registry = codelist
        .find_by_pref_term("Study Registry")
        .expect("study registry");
    assert_eq!(registry.code, "C93453");
    assert_eq!(registry.value, "Clinical Study Registry");

    let drug_company = codelist
        .find_by_value("Drug Company")
        .expect("drug company submission value");
    assert_eq!(drug_company.code, "C54149");
    assert_eq!(drug_company.pref_term, "Pharmaceutical Company");
}

#[test]
fn ddf_valid_codelist_dates_include_ddf_package_versions() {
    let operation = OperationSpec {
        fields: std::collections::BTreeMap::from([(
            "ct_package_types".to_owned(),
            serde_json::Value::Array(vec![serde_json::Value::String("DDF".to_owned())]),
        )]),
    };

    let dates = valid_codelist_dates_for_operation(&operation);

    assert!(dates.contains(&"2025-09-26"));
    assert!(dates.contains(&"2024-09-27"));
    assert!(dates.contains(&"2023-12-15"));
}

#[test]
fn ddf_study_role_terms_are_scoped_by_package_version() {
    let term = static_codelist("C215480")
        .expect("study role codelist")
        .find_by_code("C78726")
        .expect("adjudication committee");

    assert!(!static_codelist_term_matches_version(
        "C215480",
        term,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C215480",
        term,
        Some("2025-09-26")
    ));
}

#[test]
fn ddf_study_role_codelist_is_scoped_by_package_version() {
    assert!(!static_codelist_matches_version(
        "C215480",
        Some("2024-09-27")
    ));
    assert!(static_codelist_matches_version(
        "C215480",
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_trial_type_terms() {
    let codelist = static_codelist("C66739").expect("trial type codelist");
    assert!(codelist.extensible);

    let alcohol_effect = codelist
        .find_by_code("C158284")
        .expect("alcohol effect term");
    assert_eq!(alcohol_effect.value, "ALCOHOL EFFECT");
    assert_eq!(alcohol_effect.pref_term, "Alcohol Effect Study");

    let water_effect = codelist
        .find_by_value("WATER EFFECT")
        .expect("water effect submission value");
    assert_eq!(water_effect.code, "C161480");
    assert_eq!(water_effect.pref_term, "Water Effect Trial");

    let dose_response = codelist
        .find_by_pref_term("Dose Response Study")
        .expect("dose response preferred term");
    assert_eq!(dose_response.code, "C127803");
    assert_eq!(dose_response.value, "DOSE RESPONSE");
}

#[test]
fn static_codelist_resolves_sdtm_trial_intent_type_terms() {
    let codelist = static_codelist("C66736").expect("trial intent type codelist");
    assert!(codelist.extensible);

    let basic = codelist.find_by_code("C15714").expect("basic science");
    assert_eq!(basic.value, "BASIC SCIENCE");
    assert_eq!(basic.pref_term, "Basic Research");

    let mitigation = codelist
        .find_by_value("MITIGATION")
        .expect("mitigation submission value");
    assert_eq!(mitigation.code, "C49655");
    assert_eq!(mitigation.pref_term, "Adverse Effect Mitigation Study");

    let supportive = codelist
        .find_by_pref_term("Supportive Care Study")
        .expect("supportive care preferred term");
    assert_eq!(supportive.code, "C71486");
    assert_eq!(supportive.value, "SUPPORTIVE CARE");
}

#[test]
fn static_codelist_resolves_sdtm_blinding_schema_terms() {
    let codelist = static_codelist("C66735").expect("blinding schema codelist");
    assert!(codelist.extensible);

    let double_blind = codelist.find_by_code("C15228").expect("double blind");
    assert_eq!(double_blind.value, "DOUBLE BLIND");
    assert_eq!(double_blind.pref_term, "Double Blind Study");

    let open_label = codelist
        .find_by_value("OPEN LABEL")
        .expect("open label submission value");
    assert_eq!(open_label.code, "C49659");
    assert_eq!(open_label.pref_term, "Open Label Study");

    let single_blind = codelist
        .find_by_pref_term("Single Blind Study")
        .expect("single blind");
    assert_eq!(single_blind.code, "C28233");
    assert_eq!(single_blind.value, "SINGLE BLIND");
}

#[test]
fn sdtm_blinding_schema_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C66735").expect("blinding schema codelist");
    let double_blind = codelist.find_by_code("C15228").expect("double blind");
    let single_blind = codelist.find_by_code("C28233").expect("single blind");

    assert!(static_codelist_term_matches_version(
        "C66735",
        double_blind,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C66735",
        single_blind,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C66735",
        single_blind,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_intervention_model_terms() {
    let codelist = static_codelist("C99076").expect("intervention model codelist");
    assert!(codelist.extensible);

    let crossover = codelist.find_by_code("C82637").expect("crossover");
    assert_eq!(crossover.value, "CROSS-OVER");
    assert_eq!(crossover.pref_term, "Crossover Study");

    let parallel = codelist
        .find_by_value("PARALLEL")
        .expect("parallel submission value");
    assert_eq!(parallel.code, "C82639");
    assert_eq!(parallel.pref_term, "Parallel Study");

    let sequential = codelist
        .find_by_pref_term("Group Sequential Design")
        .expect("sequential preferred term");
    assert_eq!(sequential.code, "C142568");
    assert_eq!(sequential.value, "SEQUENTIAL");
}

#[test]
fn sdtm_intervention_model_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C99076").expect("intervention model codelist");
    let crossover = codelist.find_by_code("C82637").expect("crossover");
    let single_group = codelist.find_by_code("C82640").expect("single group");

    assert!(static_codelist_term_matches_version(
        "C99076",
        crossover,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99076",
        single_group,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C99076",
        single_group,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_study_type_terms() {
    let codelist = static_codelist("C99077").expect("study type codelist");
    assert!(!codelist.extensible);

    let interventional = codelist.find_by_code("C98388").expect("interventional");
    assert_eq!(interventional.value, "INTERVENTIONAL");
    assert_eq!(interventional.pref_term, "Interventional Study");

    let expanded_access = codelist
        .find_by_value("EXPANDED ACCESS")
        .expect("expanded access submission value");
    assert_eq!(expanded_access.code, "C98722");
    assert_eq!(expanded_access.pref_term, "Expanded Access Study");

    let patient_registry = codelist
        .find_by_pref_term("Patient Registry Study")
        .expect("patient registry preferred term");
    assert_eq!(patient_registry.code, "C129000");
    assert_eq!(patient_registry.value, "PATIENT REGISTRY");
}

#[test]
fn static_codelist_resolves_sdtm_route_terms() {
    let codelist = static_codelist("C66729").expect("route codelist");
    assert!(codelist.extensible);

    let oral = codelist.find_by_code("C38288").expect("oral");
    assert_eq!(oral.value, "ORAL");
    assert_eq!(oral.pref_term, "Oral Route of Administration");

    let transdermal = codelist
        .find_by_value("TRANSDERMAL")
        .expect("transdermal submission value");
    assert_eq!(transdermal.code, "C38305");
    assert_eq!(transdermal.pref_term, "Transdermal Route of Administration");

    let nasoduodenal = codelist
        .find_by_pref_term("Nasoduodenal Route of Administration")
        .expect("nasoduodenal preferred term");
    assert_eq!(nasoduodenal.code, "C188189");
    assert_eq!(nasoduodenal.value, "NASODUODENAL");
}

#[test]
fn static_codelist_resolves_sdtm_frequency_terms() {
    let codelist = static_codelist("C71113").expect("frequency codelist");
    assert!(codelist.extensible);

    let every_eighteen_hours = codelist.find_by_code("C64508").expect("q18h");
    assert_eq!(every_eighteen_hours.value, "Q18H");
    assert_eq!(every_eighteen_hours.pref_term, "Every Eighteen Hours");

    let every_other_day = codelist
        .find_by_value("QOD")
        .expect("every other day submission value");
    assert_eq!(every_other_day.code, "C64525");
    assert_eq!(every_other_day.pref_term, "Every Other Day");

    let three_times_weekly = codelist
        .find_by_pref_term("Three Times Weekly")
        .expect("three times weekly preferred term");
    assert_eq!(three_times_weekly.code, "C64528");
    assert_eq!(three_times_weekly.value, "3 TIMES PER WEEK");
}

#[test]
fn static_codelist_resolves_ddf_protocol_status_terms() {
    let codelist = static_codelist("C188723").expect("protocol status codelist");
    assert!(!codelist.extensible);

    let approved = codelist.find_by_code("C25425").expect("approved");
    assert_eq!(approved.value, "Approval");
    assert_eq!(approved.pref_term, "Approved");

    let final_status = codelist
        .find_by_value("Final")
        .expect("final submission value");
    assert_eq!(final_status.code, "C25508");
    assert_eq!(final_status.pref_term, "Final");

    let pending_review = codelist
        .find_by_pref_term("Pending Review")
        .expect("pending review preferred term");
    assert_eq!(pending_review.code, "C188862");
    assert_eq!(pending_review.value, "Pending Review");
}

#[test]
fn static_codelist_resolves_ddf_product_designation_terms_by_version() {
    let codelist = static_codelist("C207418").expect("product designation codelist");
    assert!(!codelist.extensible);

    let investigational =
        static_codelist_term_by_code("C207418", &codelist, "C202579", Some("2024-09-27"))
            .expect("investigational product");
    assert_eq!(investigational.value, "IMP");
    assert_eq!(
        investigational.pref_term,
        "Investigational Medicinal Product"
    );

    let auxiliary_2024 =
        static_codelist_term_by_code("C207418", &codelist, "C156473", Some("2024-09-27"))
            .expect("2024 auxiliary product");
    assert_eq!(auxiliary_2024.value, "NIMP (AxMP)");
    assert_eq!(auxiliary_2024.pref_term, "Auxiliary Medicinal Product");

    let auxiliary_2025 =
        static_codelist_term_by_code("C207418", &codelist, "C156473", Some("2025-09-26"))
            .expect("2025 auxiliary product");
    assert_eq!(auxiliary_2025.value, "NIMP");
    assert_eq!(auxiliary_2025.pref_term, "Auxiliary Medicinal Product");

    assert!(
        static_codelist_term_by_value("C207418", &codelist, "NIMP", Some("2024-09-27"),).is_none()
    );
    assert_eq!(
        static_codelist_term_by_value("C207418", &codelist, "NIMP", Some("2025-09-26"))
            .expect("2025 NIMP value")
            .code,
        "C156473"
    );
}

#[test]
fn static_codelist_resolves_sdtm_trial_phase_terms_by_version() {
    let codelist = static_codelist("C66737").expect("trial phase codelist");
    assert!(codelist.extensible);

    let phase_i_ii_iii_2022 =
        static_codelist_term_by_code("C66737", &codelist, "C198366", Some("2022-12-16"))
            .expect("2022 phase I/II/III");
    assert_eq!(phase_i_ii_iii_2022.value, "PHASE I/II/III STUDY");
    assert_eq!(phase_i_ii_iii_2022.pref_term, "Phase I/II/III Study");

    let phase_i_ii_iii_2023 =
        static_codelist_term_by_code("C66737", &codelist, "C198366", Some("2023-12-15"))
            .expect("2023 phase I/II/III");
    assert_eq!(phase_i_ii_iii_2023.value, "PHASE I/II/III TRIAL");
    assert_eq!(phase_i_ii_iii_2023.pref_term, "Phase I/II/III Trial");

    let early_phase_2023 =
        static_codelist_term_by_code("C66737", &codelist, "C54721", Some("2023-12-15"))
            .expect("2023 early phase");
    assert_eq!(early_phase_2023.value, "PHASE 0 TRIAL");

    let early_phase_2024 =
        static_codelist_term_by_code("C66737", &codelist, "C54721", Some("2024-09-27"))
            .expect("2024 early phase");
    assert_eq!(early_phase_2024.value, "EARLY PHASE I");
    assert_eq!(early_phase_2024.pref_term, "Early Phase 1 Trial");

    assert!(static_codelist_term_by_value(
        "C66737",
        &codelist,
        "PHASE 0 TRIAL",
        Some("2025-09-26"),
    )
    .is_none());
}

#[test]
fn static_codelist_resolves_small_oracle_value_sets() {
    let objective = static_codelist("C188725").expect("objective level codelist");
    assert!(!objective.extensible);
    assert_eq!(
        objective
            .find_by_code("C85826")
            .expect("primary objective")
            .value,
        "Study Primary Objective"
    );
    assert_eq!(
        objective
            .find_by_value("Exploratory Objective")
            .expect("exploratory objective")
            .pref_term,
        "Trial Exploratory Objective"
    );

    let endpoint = static_codelist("C188726").expect("endpoint level codelist");
    assert!(!endpoint.extensible);
    assert_eq!(
        endpoint
            .find_by_code("C94496")
            .expect("primary endpoint")
            .value,
        "Primary Endpoint"
    );
    assert_eq!(
        endpoint
            .find_by_pref_term("Exploratory Endpoint")
            .expect("exploratory endpoint")
            .code,
        "C170559"
    );

    let geographic_scope = static_codelist("C207412").expect("geographic scope codelist");
    assert!(!geographic_scope.extensible);
    assert_eq!(
        geographic_scope
            .find_by_code("C25464")
            .expect("country")
            .value,
        "Country"
    );
    assert_eq!(
        geographic_scope
            .find_by_value("Global")
            .expect("global")
            .code,
        "C68846"
    );

    let eligibility_category = static_codelist("C66797").expect("eligibility category codelist");
    assert!(!eligibility_category.extensible);
    assert_eq!(
        eligibility_category
            .find_by_value("EXCLUSION")
            .expect("exclusion")
            .pref_term,
        "Exclusion Criteria"
    );

    let encounter_type = static_codelist("C188728").expect("encounter type codelist");
    assert!(encounter_type.extensible);
    assert_eq!(
        encounter_type.find_by_code("C25716").expect("visit").value,
        "Visit"
    );
}

#[test]
fn static_codelist_resolves_additional_oracle_value_sets() {
    let sampling = static_codelist("C127260").expect("sampling method codelist");
    assert!(sampling.extensible);
    assert!(!static_codelist_matches_version(
        "C127260",
        Some("2016-03-25")
    ));
    assert!(static_codelist_matches_version(
        "C127260",
        Some("2024-09-27")
    ));
    assert_eq!(
        sampling
            .find_by_value("NON-PROBABILITY SAMPLE")
            .expect("non-probability sample")
            .pref_term,
        "Non-Probability Sampling Method"
    );

    let perspective = static_codelist("C127261").expect("time perspective codelist");
    assert!(perspective.extensible);
    assert!(!static_codelist_matches_version(
        "C127261",
        Some("2016-03-25")
    ));
    assert!(static_codelist_matches_version(
        "C127261",
        Some("2024-09-27")
    ));
    assert_eq!(
        perspective
            .find_by_value("RETROSPECTIVE")
            .expect("retrospective")
            .pref_term,
        "Retrospective Study"
    );

    let timing_type = static_codelist("C201264").expect("timing type codelist");
    assert!(!timing_type.extensible);
    assert_eq!(
        timing_type
            .find_by_pref_term("Fixed Reference Timing Type")
            .expect("fixed reference")
            .value,
        "Fixed Reference"
    );

    let governance_date = static_codelist("C207413").expect("governance date codelist");
    assert!(governance_date.extensible);
    assert_eq!(
        governance_date
            .find_by_value("Sponsor Approval Date")
            .expect("sponsor approval")
            .pref_term,
        "Protocol Approval by Sponsor Date"
    );

    let title_type = static_codelist("C207419").expect("study title type codelist");
    assert!(!title_type.extensible);
    assert_eq!(
        title_type
            .find_by_pref_term("Scientific Study Title")
            .expect("scientific title")
            .code,
        "C207618"
    );

    let definition_document =
        static_codelist("C215477").expect("study definition document type codelist");
    assert!(definition_document.extensible);
    assert_eq!(
        definition_document
            .find_by_value("Protocol")
            .expect("protocol")
            .pref_term,
        "Study Protocol"
    );

    let reference_identifier =
        static_codelist("C215478").expect("reference identifier type codelist");
    assert!(reference_identifier.extensible);
    assert_eq!(
        reference_identifier
            .find_by_pref_term("Pediatric Investigation Plan")
            .expect("pediatric investigation plan")
            .value,
        "Pediatric Investigation Clinical Development Plan"
    );

    let product_property = static_codelist("C215479").expect("product property type codelist");
    assert!(product_property.extensible);
    assert_eq!(
        product_property.find_by_code("C45997").expect("ph").value,
        "pH"
    );

    let amendment_impact = static_codelist("C215481").expect("amendment impact codelist");
    assert!(amendment_impact.extensible);
    assert_eq!(
        amendment_impact
            .find_by_value("Study Data Robustness")
            .expect("robustness")
            .code,
        "C215668"
    );

    let medical_device_sourcing =
        static_codelist("C215482").expect("medical device sourcing codelist");
    assert!(medical_device_sourcing.extensible);
    assert_eq!(
        medical_device_sourcing
            .find_by_value("Locally Sourced")
            .expect("locally sourced")
            .pref_term,
        "Locally Sourced Indicator"
    );
    let product_sourcing = static_codelist("C215483").expect("product sourcing codelist");
    assert!(product_sourcing.extensible);
    assert_eq!(
        product_sourcing
            .find_by_pref_term("Centrally Sourced Indicator")
            .expect("centrally sourced")
            .value,
        "Centrally Sourced"
    );

    let device_identifier = static_codelist("C215484").expect("device identifier type codelist");
    assert!(device_identifier.extensible);
    assert_eq!(
        device_identifier
            .find_by_value("FDA Unique Device Identification")
            .expect("fda udi")
            .pref_term,
        "FDA Unique Device Identifier"
    );

    let dosage_form = static_codelist("C66726").expect("dosage form codelist");
    assert!(dosage_form.extensible);
    assert_eq!(
        dosage_form
            .find_by_value("TABLET")
            .expect("tablet")
            .pref_term,
        "Tablet Dosage Form"
    );

    let timing_relative = static_codelist("C201265").expect("timing relative codelist");
    assert!(!timing_relative.extensible);
    assert_eq!(
        timing_relative
            .find_by_value("End to Start")
            .expect("end to start")
            .code,
        "C201353"
    );

    let masking_role = static_codelist("C207414").expect("masking role codelist");
    assert!(masking_role.extensible);
    assert!(static_codelist_matches_version(
        "C207414",
        Some("2024-09-27")
    ));
    assert!(!static_codelist_matches_version(
        "C207414",
        Some("2025-09-26")
    ));
    assert_eq!(
        masking_role
            .find_by_pref_term("Clinical Study Sponsor")
            .expect("clinical sponsor")
            .value,
        "Sponsor"
    );

    let data_origin = static_codelist("C188727").expect("data origin type codelist");
    assert!(data_origin.extensible);
    assert_eq!(
        data_origin
            .find_by_pref_term("Synthetic Data")
            .expect("synthetic data")
            .code,
        "C176263"
    );
    let real_world_2024 =
        static_codelist_term_by_code("C188727", &data_origin, "C165830", Some("2024-09-27"))
            .expect("2024 real world data");
    assert_eq!(real_world_2024.value, "Real World Data");
    let real_world_2025 =
        static_codelist_term_by_code("C188727", &data_origin, "C165830", Some("2025-09-26"))
            .expect("2025 real-world data");
    assert_eq!(real_world_2025.value, "Real-world Data");
}

#[test]
fn static_codelist_resolves_sdtm_environmental_setting_terms() {
    let codelist = static_codelist("C127262").expect("environmental setting codelist");
    assert!(codelist.extensible);

    let childcare = codelist
        .find_by_code("C127785")
        .expect("childcare center term");
    assert_eq!(childcare.value, "CHILD CARE CENTER");
    assert_eq!(childcare.pref_term, "Childcare Center");

    let outpatient = codelist
        .find_by_value("OUTPATIENT CLINIC")
        .expect("outpatient clinic submission value");
    assert_eq!(outpatient.code, "C16281");
    assert_eq!(outpatient.pref_term, "Ambulatory Care Facility");

    let correctional = codelist
        .find_by_pref_term("Correctional Institution")
        .expect("correctional institution preferred term");
    assert_eq!(correctional.code, "C85862");
    assert_eq!(correctional.value, "PRISON");
}

#[test]
fn static_codelist_resolves_sdtm_contact_mode_terms() {
    let codelist = static_codelist("C171445").expect("contact mode codelist");
    assert!(codelist.extensible);

    let email = codelist.find_by_code("C25170").expect("email");
    assert_eq!(email.value, "E-MAIL");
    assert_eq!(email.pref_term, "E-mail");

    let remote_audio_video = codelist
        .find_by_value("REMOTE AUDIO VIDEO")
        .expect("remote audio video submission value");
    assert_eq!(remote_audio_video.code, "C171525");
    assert_eq!(remote_audio_video.pref_term, "Audio-Videoconferencing");

    let ivrs = codelist
        .find_by_pref_term("Interactive Voice Response System")
        .expect("ivrs preferred term");
    assert_eq!(ivrs.code, "C177933");
    assert_eq!(ivrs.value, "IVRS");
}

#[test]
fn static_codelist_resolves_sdtm_age_unit_terms() {
    let codelist = static_codelist("C66781").expect("age unit codelist");
    assert!(!codelist.extensible);

    let hour = codelist.find_by_code("C25529").expect("hour");
    assert_eq!(hour.value, "HOURS");
    assert_eq!(hour.pref_term, "Hour");

    let year = codelist
        .find_by_value("YEARS")
        .expect("years submission value");
    assert_eq!(year.code, "C29848");
    assert_eq!(year.pref_term, "Year");

    let month = codelist.find_by_pref_term("Month").expect("month");
    assert_eq!(month.code, "C29846");
    assert_eq!(month.value, "MONTHS");
}

#[test]
fn static_codelist_resolves_sdtm_unit_terms() {
    let codelist = static_codelist("C71620").expect("unit codelist");
    assert!(codelist.extensible);

    let day = codelist.find_by_code("C25301").expect("day");
    assert_eq!(day.value, "DAYS");
    assert_eq!(day.pref_term, "Day");

    let milligram = codelist.find_by_value("mg").expect("milligram");
    assert_eq!(milligram.code, "C28253");
    assert_eq!(milligram.pref_term, "Milligram");

    let microvolt_second = codelist
        .find_by_pref_term("Microvolt Second")
        .expect("microvolt second");
    assert_eq!(microvolt_second.code, "C105499");
    assert_eq!(microvolt_second.value, "uV*s");
}

#[test]
fn sdtm_unit_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C71620").expect("unit codelist");
    let day = codelist.find_by_code("C25301").expect("day");
    let microvolt_second = codelist.find_by_code("C105499").expect("microvolt second");
    let per_day = codelist.find_by_code("C176378").expect("per day");

    assert!(static_codelist_term_matches_version(
        "C71620",
        day,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C71620",
        microvolt_second,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C71620",
        microvolt_second,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C71620",
        per_day,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C71620",
        per_day,
        Some("2025-09-26")
    ));
}

#[test]
fn sdtm_contact_mode_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C171445").expect("contact mode codelist");
    let email = codelist.find_by_code("C25170").expect("email");
    let ivrs = codelist.find_by_code("C177933").expect("ivrs");

    assert!(static_codelist_term_matches_version(
        "C171445",
        email,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C171445",
        ivrs,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C171445",
        ivrs,
        Some("2024-03-29")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_intervention_type_terms() {
    let codelist = static_codelist("C99078").expect("intervention type codelist");
    assert!(!codelist.extensible);

    let behavioral = codelist.find_by_code("C15184").expect("behavioral");
    assert_eq!(behavioral.value, "BEHAVIORAL THERAPY");
    assert_eq!(behavioral.pref_term, "Behavioral Intervention");

    let device = codelist
        .find_by_value("DEVICE")
        .expect("device submission value");
    assert_eq!(device.code, "C16830");
    assert_eq!(device.pref_term, "Medical Device");

    let procedure = codelist
        .find_by_pref_term("Physical Medical Procedure")
        .expect("procedure preferred term");
    assert_eq!(procedure.code, "C98769");
    assert_eq!(procedure.value, "PROCEDURE");
}

#[test]
fn sdtm_intervention_type_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C99078").expect("intervention type codelist");
    let combination = codelist
        .find_by_code("C54696")
        .expect("combination product");
    let non_surgical = codelist
        .find_by_code("C218507")
        .expect("non-surgical procedure");
    let other = codelist.find_by_code("C17649").expect("other");

    assert!(!static_codelist_term_matches_version(
        "C99078",
        combination,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        combination,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99078",
        non_surgical,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        non_surgical,
        Some("2025-09-26")
    ));
    assert!(static_codelist_term_matches_version(
        "C99078",
        other,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C99078",
        other,
        Some("2025-03-28")
    ));
}

#[test]
fn static_codelist_resolves_sdtm_observational_model_terms() {
    let codelist = static_codelist("C127259").expect("observational model codelist");
    assert!(codelist.extensible);

    let case_control = codelist.find_by_code("C15197").expect("case control");
    assert_eq!(case_control.value, "CASE CONTROL");
    assert_eq!(case_control.pref_term, "Case-Control Study");

    let cohort = codelist
        .find_by_value("COHORT")
        .expect("cohort submission value");
    assert_eq!(cohort.code, "C15208");
    assert_eq!(cohort.pref_term, "Cohort Study");

    let family = codelist.find_by_pref_term("Family Study").expect("family");
    assert_eq!(family.code, "C15407");
    assert_eq!(family.value, "FAMILY BASED");
}

#[test]
fn sdtm_observational_model_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C127259").expect("observational model codelist");
    let case_control = codelist.find_by_code("C15197").expect("case control");
    let cohort = codelist.find_by_code("C15208").expect("cohort");
    let ecologic = codelist.find_by_code("C127780").expect("ecologic");

    assert!(!static_codelist_matches_version(
        "C127259",
        Some("2016-03-25")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        case_control,
        Some("2023-12-15")
    ));
    assert!(!static_codelist_term_matches_version(
        "C127259",
        cohort,
        Some("2023-12-15")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        cohort,
        Some("2024-03-29")
    ));
    assert!(!static_codelist_term_matches_version(
        "C127259",
        ecologic,
        Some("2024-03-29")
    ));
    assert!(static_codelist_term_matches_version(
        "C127259",
        ecologic,
        Some("2024-09-27")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_role_terms() {
    let codelist = static_codelist("C215480").expect("study role codelist");
    assert!(codelist.extensible);

    let care_provider = codelist.find_by_code("C17445").expect("care provider term");
    assert_eq!(care_provider.value, "Care Provider");
    assert_eq!(care_provider.pref_term, "Caregiver");

    let co_sponsor = codelist
        .find_by_value("Co-Sponsor")
        .expect("co-sponsor submission value");
    assert_eq!(co_sponsor.code, "C215669");
    assert_eq!(co_sponsor.pref_term, "Study Co-Sponsor");

    let clinical_sponsor = codelist
        .find_by_pref_term("Clinical Study Sponsor")
        .expect("clinical study sponsor preferred term");
    assert_eq!(clinical_sponsor.code, "C70793");
    assert_eq!(clinical_sponsor.value, "Sponsor");
}

#[test]
fn static_codelist_resolves_ddf_study_amendment_reason_terms() {
    let codelist = static_codelist("C207415").expect("study amendment reason codelist");
    assert!(!codelist.extensible);

    let standard_of_care = codelist
        .find_by_code("C207600")
        .expect("change in standard of care");
    assert_eq!(standard_of_care.value, "Change In Standard Of Care");
    assert_eq!(standard_of_care.pref_term, "Change In Standard Of Care");

    let other = codelist
        .find_by_value("OTHER")
        .expect("other submission value");
    assert_eq!(other.code, "C17649");
    assert_eq!(other.pref_term, "Other");

    let extension = codelist
        .find_by_pref_term("Extension")
        .expect("extension preferred term");
    assert_eq!(extension.code, "C0031X");
    assert_eq!(extension.value, "Extension");
}

#[test]
fn ddf_study_amendment_reason_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207415").expect("study amendment reason codelist");
    let standard_of_care = codelist
        .find_by_code("C207600")
        .expect("change in standard of care");
    let other = codelist.find_by_code("C17649").expect("other");

    assert!(static_codelist_term_matches_version(
        "C207415",
        standard_of_care,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207415",
        other,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207415",
        other,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_design_characteristic_terms() {
    let codelist = static_codelist("C207416").expect("study design characteristic codelist");
    assert!(codelist.extensible);

    let randomized = codelist.find_by_code("C46079").expect("randomized");
    assert_eq!(randomized.value, "Randomized");
    assert_eq!(randomized.pref_term, "Randomized Controlled Clinical Trial");

    let single_centre = codelist
        .find_by_value("Single-Centre")
        .expect("single-centre submission value");
    assert_eq!(single_centre.code, "C217004");
    assert_eq!(single_centre.pref_term, "Single-Center Study");

    let stratified = codelist
        .find_by_pref_term("Stratified Randomization")
        .expect("stratified randomization preferred term");
    assert_eq!(stratified.code, "C147145");
    assert_eq!(stratified.value, "Stratified Randomisation");
}

#[test]
fn ddf_study_design_characteristic_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207416").expect("study design characteristic codelist");
    let randomized = codelist.find_by_code("C46079").expect("randomized");
    let single_centre = codelist.find_by_code("C217004").expect("single-centre");

    assert!(!static_codelist_matches_version(
        "C207416",
        Some("2023-12-15")
    ));
    assert!(static_codelist_matches_version(
        "C207416",
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207416",
        randomized,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207416",
        single_centre,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207416",
        single_centre,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_study_intervention_role_terms() {
    let codelist = static_codelist("C207417").expect("study intervention role codelist");
    assert!(!codelist.extensible);

    let required = codelist
        .find_by_code("C207614")
        .expect("additional required treatment");
    assert_eq!(required.value, "Additional Required Treatment");
    assert_eq!(required.pref_term, "Additional Required Medicinal Product");

    let diagnostic = codelist
        .find_by_value("Diagnostic")
        .expect("diagnostic submission value");
    assert_eq!(diagnostic.code, "C18020");
    assert_eq!(diagnostic.pref_term, "Diagnostic Procedure");

    let rescue = codelist
        .find_by_pref_term("Rescue Medications")
        .expect("rescue preferred term");
    assert_eq!(rescue.code, "C165835");
    assert_eq!(rescue.value, "Rescue Medicine");
}

#[test]
fn ddf_study_intervention_role_terms_are_scoped_by_package_version() {
    let codelist = static_codelist("C207417").expect("study intervention role codelist");
    let placebo = codelist.find_by_code("C753").expect("placebo");
    let active = codelist.find_by_code("C68609").expect("active comparator");

    assert!(static_codelist_term_matches_version(
        "C207417",
        placebo,
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C207417",
        active,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C207417",
        active,
        Some("2025-09-26")
    ));
}

#[test]
fn static_codelist_resolves_ddf_observational_study_subtype_terms() {
    let codelist = static_codelist("C215486").expect("observational subtype codelist");
    assert!(codelist.extensible);

    let education = codelist
        .find_by_code("C215657")
        .expect("clinical education");
    assert_eq!(education.value, "Clinical Education");
    assert_eq!(education.pref_term, "Clinical Education Study");

    let prevalence = codelist
        .find_by_value("Disease Prevalence")
        .expect("disease prevalence submission value");
    assert_eq!(prevalence.code, "C215675");
    assert_eq!(prevalence.pref_term, "Disease Prevalence Study");

    let safety = codelist
        .find_by_pref_term("Safety Study")
        .expect("safety preferred term");
    assert_eq!(safety.code, "C49667");
    assert_eq!(safety.value, "Safety");
}

#[test]
fn ddf_observational_study_subtype_codelist_is_scoped_by_package_version() {
    let term = static_codelist("C215486")
        .expect("observational subtype codelist")
        .find_by_code("C215657")
        .expect("clinical education");

    assert!(!static_codelist_matches_version(
        "C215486",
        Some("2024-09-27")
    ));
    assert!(!static_codelist_term_matches_version(
        "C215486",
        term,
        Some("2024-09-27")
    ));
    assert!(static_codelist_term_matches_version(
        "C215486",
        term,
        Some("2025-09-26")
    ));
}
