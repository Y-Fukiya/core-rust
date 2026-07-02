use core_rule_model::ExecutableRule;

pub(crate) const CORE_000015: &str = "CORE-000015";
pub(crate) const CORE_000007: &str = "CORE-000007";
pub(crate) const CORE_000025: &str = "CORE-000025";
pub(crate) const CORE_000037: &str = "CORE-000037";
pub(crate) const CORE_000038: &str = "CORE-000038";
pub(crate) const CORE_000044: &str = "CORE-000044";
pub(crate) const CORE_000047: &str = "CORE-000047";
pub(crate) const CORE_000039: &str = "CORE-000039";
pub(crate) const CORE_000095: &str = "CORE-000095";
pub(crate) const CORE_000096: &str = "CORE-000096";
pub(crate) const CORE_000098: &str = "CORE-000098";
pub(crate) const CORE_000124: &str = "CORE-000124";
pub(crate) const CORE_000138: &str = "CORE-000138";
pub(crate) const CORE_000139: &str = "CORE-000139";
pub(crate) const CORE_000140: &str = "CORE-000140";
pub(crate) const CORE_000142: &str = "CORE-000142";
pub(crate) const CORE_000147: &str = "CORE-000147";
pub(crate) const CORE_000148: &str = "CORE-000148";
pub(crate) const CORE_000165: &str = "CORE-000165";
pub(crate) const CORE_000166: &str = "CORE-000166";
pub(crate) const CORE_000167: &str = "CORE-000167";
pub(crate) const CORE_000177: &str = "CORE-000177";
pub(crate) const CORE_000185: &str = "CORE-000185";
pub(crate) const CORE_000186: &str = "CORE-000186";
pub(crate) const CORE_000187: &str = "CORE-000187";
pub(crate) const CORE_000201: &str = "CORE-000201";
pub(crate) const CORE_000223: &str = "CORE-000223";
pub(crate) const CORE_000224: &str = "CORE-000224";
pub(crate) const CORE_000228: &str = "CORE-000228";
pub(crate) const CORE_000206: &str = "CORE-000206";
pub(crate) const CORE_000269: &str = "CORE-000269";
pub(crate) const CORE_000270: &str = "CORE-000270";
pub(crate) const CORE_000272: &str = "CORE-000272";
pub(crate) const CORE_000321: &str = "CORE-000321";
pub(crate) const CORE_000324: &str = "CORE-000324";
pub(crate) const CORE_000328: &str = "CORE-000328";
pub(crate) const CORE_000361: &str = "CORE-000361";
pub(crate) const CORE_000376: &str = "CORE-000376";
pub(crate) const CORE_000390: &str = "CORE-000390";
pub(crate) const CORE_000398: &str = "CORE-000398";
pub(crate) const CORE_000454: &str = "CORE-000454";
pub(crate) const CORE_000460: &str = "CORE-000460";
pub(crate) const CORE_000466: &str = "CORE-000466";
pub(crate) const CORE_000494: &str = "CORE-000494";
pub(crate) const CORE_000495: &str = "CORE-000495";
pub(crate) const CORE_000505: &str = "CORE-000505";
pub(crate) const CORE_000507: &str = "CORE-000507";
pub(crate) const CORE_000516: &str = "CORE-000516";
pub(crate) const CORE_000526: &str = "CORE-000526";
pub(crate) const CORE_000539: &str = "CORE-000539";
pub(crate) const CORE_000540: &str = "CORE-000540";
pub(crate) const CORE_000547: &str = "CORE-000547";
pub(crate) const CORE_000550: &str = "CORE-000550";
pub(crate) const CORE_000572: &str = "CORE-000572";
pub(crate) const CORE_000583: &str = "CORE-000583";
pub(crate) const CORE_000595: &str = "CORE-000595";
pub(crate) const CORE_000597: &str = "CORE-000597";
pub(crate) const CORE_000651: &str = "CORE-000651";
pub(crate) const CORE_000653: &str = "CORE-000653";
pub(crate) const CORE_000654: &str = "CORE-000654";
pub(crate) const CORE_000660: &str = "CORE-000660";
pub(crate) const CORE_000670: &str = "CORE-000670";
pub(crate) const CORE_000676: &str = "CORE-000676";
pub(crate) const CORE_000677: &str = "CORE-000677";
pub(crate) const CORE_000678: &str = "CORE-000678";
pub(crate) const CORE_000690: &str = "CORE-000690";
pub(crate) const CORE_000700: &str = "CORE-000700";
pub(crate) const CORE_000711: &str = "CORE-000711";
pub(crate) const CORE_000714: &str = "CORE-000714";
pub(crate) const CORE_000744: &str = "CORE-000744";
pub(crate) const CORE_000757: &str = "CORE-000757";
pub(crate) const CORE_000750: &str = "CORE-000750";
pub(crate) const CORE_000783: &str = "CORE-000783";
pub(crate) const CORE_000786: &str = "CORE-000786";
pub(crate) const CORE_000793: &str = "CORE-000793";
pub(crate) const CORE_000794: &str = "CORE-000794";
pub(crate) const CORE_000847: &str = "CORE-000847";
pub(crate) const CORE_000848: &str = "CORE-000848";
pub(crate) const CORE_000852: &str = "CORE-000852";
pub(crate) const CORE_000853: &str = "CORE-000853";
pub(crate) const CORE_000862: &str = "CORE-000862";
pub(crate) const CORE_000864: &str = "CORE-000864";
pub(crate) const CORE_000866: &str = "CORE-000866";
pub(crate) const CORE_000867: &str = "CORE-000867";
pub(crate) const CORE_000878: &str = "CORE-000878";
pub(crate) const CORE_000884: &str = "CORE-000884";
pub(crate) const CORE_000893: &str = "CORE-000893";
pub(crate) const CORE_000890: &str = "CORE-000890";
pub(crate) const CORE_000896: &str = "CORE-000896";
pub(crate) const CORE_000902: &str = "CORE-000902";
pub(crate) const CORE_000903: &str = "CORE-000903";
pub(crate) const CORE_000929: &str = "CORE-000929";
pub(crate) const CORE_000947: &str = "CORE-000947";
pub(crate) const CORE_001023: &str = "CORE-001023";
pub(crate) const CORE_001045: &str = "CORE-001045";

pub(crate) fn is_zb_issue_normalization_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000929
}

pub(crate) fn is_date_issue_variable_expansion_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000138
            | CORE_000139
            | CORE_000324
            | CORE_000505
            | CORE_000572
            | CORE_000653
            | CORE_000711
            | CORE_000714
            | CORE_000866
    )
}

pub(crate) fn is_tx_variable_expansion_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000460
}

pub(crate) fn is_missing_casno_oracle_issue_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000595
}

pub(crate) fn is_suppae_aesosp_parent_record_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000597
}

pub(crate) fn is_unscheduled_death_ds_flag_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000670
}

pub(crate) fn is_dataset_level_presence_result_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        "CORE-000012"
            | "CORE-000029"
            | "CORE-000048"
            | "CORE-000050"
            | "CORE-000051"
            | "CORE-000052"
            | "CORE-000054"
            | "CORE-000055"
            | "CORE-000056"
            | "CORE-000057"
            | "CORE-000058"
            | "CORE-000059"
            | "CORE-000060"
            | "CORE-000061"
            | "CORE-000064"
            | "CORE-000065"
            | "CORE-000066"
            | "CORE-000067"
            | "CORE-000068"
            | "CORE-000069"
            | "CORE-000070"
            | "CORE-000071"
            | "CORE-000111"
            | "CORE-000112"
            | "CORE-000113"
            | "CORE-000114"
            | "CORE-000242"
            | "CORE-000245"
            | "CORE-000246"
            | "CORE-000247"
            | "CORE-000258"
            | "CORE-000613"
            | "CORE-000614"
            | "CORE-000615"
            | "CORE-000617"
            | "CORE-000621"
            | "CORE-000622"
            | "CORE-000623"
            | "CORE-000624"
            | "CORE-000625"
            | "CORE-000626"
            | "CORE-000627"
            | "CORE-000628"
            | "CORE-000629"
            | "CORE-000630"
            | "CORE-000631"
            | "CORE-000632"
            | "CORE-000633"
            | "CORE-000634"
            | "CORE-000635"
            | "CORE-000636"
            | "CORE-000637"
            | "CORE-000638"
            | "CORE-000639"
            | "CORE-000640"
            | "CORE-000641"
            | "CORE-000649"
            | "CORE-000661"
            | "CORE-000662"
            | "CORE-000663"
            | "CORE-000664"
            | "CORE-000665"
            | "CORE-000667"
            | "CORE-000668"
            | "CORE-000669"
            | "CORE-000762"
            | "CORE-000788"
            | "CORE-000789"
    )
}

pub(crate) fn uses_missing_column_once_result(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000015 | CORE_000096 | CORE_000098 | CORE_000166
    )
}

pub(crate) fn uses_missing_column_zero_record_result(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000165
}

pub(crate) fn omits_unique_set_group_locator_variables(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000495
}

pub(crate) fn includes_unique_set_subject_locator_variable(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000526
}

pub(crate) fn preserves_simple_any_study_day_output_variables(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000516
}

pub(crate) fn uses_dataset_level_existing_study_day_variable_result(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000321 | CORE_000328 | CORE_000700 | CORE_000793 | CORE_000862 | CORE_000864
    )
}

pub(crate) fn includes_single_match_dataset_as_target(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000269 | CORE_000270 | CORE_000853
    )
}

pub(crate) fn uses_first_row_dataset_presence_result(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000167
}

pub(crate) fn uses_missing_scoped_dataset_presence_result(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000786
}

pub(crate) fn uses_csv_line_record_numbers(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000007
            | CORE_000025
            | CORE_000037
            | CORE_000038
            | CORE_000044
            | CORE_000124
            | CORE_000140
            | CORE_000147
            | CORE_000148
            | CORE_000185
            | CORE_000186
            | CORE_000187
    )
}

pub(crate) fn uses_previous_record_numbers(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000177 | CORE_000223 | CORE_000224 | CORE_000228
    )
}

pub(crate) fn uses_alphanumeric_fa_split_dataset_name_regex(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000540
}

pub(crate) fn is_missing_cm_dtc_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000324
}

pub(crate) fn is_elapsed_time_consistency_precondition_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000142
}

pub(crate) fn is_cv_unique_evaluation_interval_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000390
}

pub(crate) fn assumes_missing_svpresp_is_planned(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000039
}

pub(crate) fn is_trial_summary_null_flavor_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000583
}

pub(crate) fn open_rules_relationship_direction(rule: &ExecutableRule) -> Option<&'static str> {
    match rule.core_id.as_str() {
        CORE_000361 => Some("target_to_comparator"),
        CORE_000690 => Some("comparator_to_target"),
        _ => None,
    }
}

pub(crate) fn is_operation_report_variable_override_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000783
}

pub(crate) fn is_reference_distinct_report_variable_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000047
}

pub(crate) fn is_group_level_distinct_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000893
}

pub(crate) fn is_duplicate_intent_type_group_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_001023
}

pub(crate) fn is_study_arm_invalid_population_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_001045
}

pub(crate) fn uses_check_target_report_variable_for_ex_end_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000454
}

pub(crate) fn is_requested_standard_operation_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000272
}

pub(crate) fn supports_column_ref_metadata_comparator(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000852
}

pub(crate) fn is_domain_codelist_metadata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000929
}

pub(crate) fn is_library_variable_metadata_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000398 | CORE_000494 | CORE_000507 | CORE_000903 | CORE_000929
    )
}

pub(crate) fn can_skip_metadata_column_ref_comparator(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), CORE_000494 | CORE_000929)
}

pub(crate) fn is_supported_value_metadata_rule_id(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), CORE_000867 | CORE_000890)
}

pub(crate) fn is_model_column_order_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000550 | CORE_000852 | CORE_000902 | CORE_000947
    )
}

pub(crate) fn is_variable_metadata_domain_prefix_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000376
}

pub(crate) fn is_split_dataset_parent_metadata_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), CORE_000539 | CORE_000540)
}

pub(crate) fn is_forbidden_send_domain_placeholder_variable_rule(rule: &ExecutableRule) -> bool {
    matches!(
        rule.core_id.as_str(),
        CORE_000794 | CORE_000847 | CORE_000848
    )
}

pub(crate) fn is_missing_split_parent_dataset_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000539
}

pub(crate) fn is_missing_findings_about_parent_dataset_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000540
}

pub(crate) fn is_define_role_metadata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000494
}

pub(crate) fn is_library_domain_codelist_metadata_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000929
}

pub(crate) fn uses_library_variable_name_projection(rule_id: &str) -> bool {
    rule_id == CORE_000903
}

pub(crate) fn uses_library_variable_label_projection(rule_id: &str) -> bool {
    rule_id == CORE_000398
}

pub(crate) fn is_define_variable_label_projection_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000507
}

pub(crate) fn is_model_column_order_from_library_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000852
}

pub(crate) fn is_absent_reference_distinct_source_pass_through_rule(
    rule: &ExecutableRule,
    source_name: &str,
) -> bool {
    (rule.core_id == CORE_000678 && source_name.eq_ignore_ascii_case("POOLDEF"))
        || (rule.core_id == CORE_000660 && source_name.eq_ignore_ascii_case("TO"))
        || (rule.core_id == CORE_000676 && source_name.eq_ignore_ascii_case("TO"))
}

pub(crate) fn is_scope_wide_reference_target_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000201
}

pub(crate) fn is_tpt_relationship_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), CORE_000651 | CORE_000654)
}

pub(crate) fn is_dm_dataset_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000138
}

pub(crate) fn is_se_dataset_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000095
}

pub(crate) fn is_cm_dataset_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000572
}

pub(crate) fn is_pp_dataset_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000466
}

pub(crate) fn is_pooldef_poolid_oracle_result_rule(rule: &ExecutableRule) -> bool {
    matches!(rule.core_id.as_str(), CORE_000677 | CORE_000896)
}

pub(crate) fn is_idvarval_rdomain_reference_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000206
}

pub(crate) fn is_relrec_faobj_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000744
}

pub(crate) fn is_intervention_relrec_faobj_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000757
}

pub(crate) fn is_missing_column_probe_exception(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000547
}
