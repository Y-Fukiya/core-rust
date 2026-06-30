use core_rule_model::ExecutableRule;

pub(crate) const CORE_000047: &str = "CORE-000047";
pub(crate) const CORE_000095: &str = "CORE-000095";
pub(crate) const CORE_000138: &str = "CORE-000138";
pub(crate) const CORE_000139: &str = "CORE-000139";
pub(crate) const CORE_000201: &str = "CORE-000201";
pub(crate) const CORE_000272: &str = "CORE-000272";
pub(crate) const CORE_000324: &str = "CORE-000324";
pub(crate) const CORE_000361: &str = "CORE-000361";
pub(crate) const CORE_000376: &str = "CORE-000376";
pub(crate) const CORE_000398: &str = "CORE-000398";
pub(crate) const CORE_000460: &str = "CORE-000460";
pub(crate) const CORE_000466: &str = "CORE-000466";
pub(crate) const CORE_000494: &str = "CORE-000494";
pub(crate) const CORE_000505: &str = "CORE-000505";
pub(crate) const CORE_000507: &str = "CORE-000507";
pub(crate) const CORE_000539: &str = "CORE-000539";
pub(crate) const CORE_000540: &str = "CORE-000540";
pub(crate) const CORE_000547: &str = "CORE-000547";
pub(crate) const CORE_000550: &str = "CORE-000550";
pub(crate) const CORE_000572: &str = "CORE-000572";
pub(crate) const CORE_000583: &str = "CORE-000583";
pub(crate) const CORE_000595: &str = "CORE-000595";
pub(crate) const CORE_000651: &str = "CORE-000651";
pub(crate) const CORE_000653: &str = "CORE-000653";
pub(crate) const CORE_000654: &str = "CORE-000654";
pub(crate) const CORE_000677: &str = "CORE-000677";
pub(crate) const CORE_000678: &str = "CORE-000678";
pub(crate) const CORE_000711: &str = "CORE-000711";
pub(crate) const CORE_000714: &str = "CORE-000714";
pub(crate) const CORE_000744: &str = "CORE-000744";
pub(crate) const CORE_000783: &str = "CORE-000783";
pub(crate) const CORE_000852: &str = "CORE-000852";
pub(crate) const CORE_000866: &str = "CORE-000866";
pub(crate) const CORE_000867: &str = "CORE-000867";
pub(crate) const CORE_000890: &str = "CORE-000890";
pub(crate) const CORE_000902: &str = "CORE-000902";
pub(crate) const CORE_000903: &str = "CORE-000903";
pub(crate) const CORE_000929: &str = "CORE-000929";
pub(crate) const CORE_000947: &str = "CORE-000947";

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

pub(crate) fn is_missing_cm_dtc_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000324
}

pub(crate) fn is_trial_summary_null_flavor_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000583
}

pub(crate) fn is_open_rules_relationship_direction_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000361
}

pub(crate) fn is_operation_report_variable_override_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000783
}

pub(crate) fn is_reference_distinct_report_variable_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000047
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
    rule.core_id == CORE_000678 && source_name.eq_ignore_ascii_case("POOLDEF")
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
    rule.core_id == CORE_000677
}

pub(crate) fn is_relrec_faobj_oracle_result_rule(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000744
}

pub(crate) fn is_missing_column_probe_exception(rule: &ExecutableRule) -> bool {
    rule.core_id == CORE_000547
}
