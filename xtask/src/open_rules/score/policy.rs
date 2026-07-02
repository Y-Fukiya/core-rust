use crate::open_rules::discovery::OpenRulesCase;

pub(super) fn official_oracle_fixture_gap_category(case: &OpenRulesCase) -> bool {
    core_api::rule_id_has_oracle_gap_category(&case.rule_id, "official_oracle_fixture_gap")
}

pub(super) fn row_locator_oracle_gap_category(case: &OpenRulesCase) -> bool {
    [
        "record_row_locator",
        "defer_unique_set",
        "scope_wide_reference_distinct",
        "defer_duplicate_match_dataset",
    ]
    .into_iter()
    .any(|category| core_api::rule_id_has_oracle_gap_category(&case.rule_id, category))
}

pub(super) fn output_context_variable_oracle_gap_category(case: &OpenRulesCase) -> bool {
    [
        "defer_empty_non_empty",
        "empty_non_empty",
        "defer_positive_zero_probe",
    ]
    .into_iter()
    .any(|category| core_api::rule_id_has_oracle_gap_category(&case.rule_id, category))
}

pub(super) fn deferred_default_engine_oracle_gap_reason(case: &OpenRulesCase) -> Option<String> {
    let reason = deferred_default_engine_oracle_gap_reason_text(case)?;
    Some(format!(
        "{reason}; excluded from supported accuracy until native semantics are verified"
    ))
}

pub(super) fn deferred_default_engine_oracle_gap_skipped_reason(
    case: &OpenRulesCase,
) -> Option<String> {
    let reason = deferred_default_engine_oracle_gap_reason_text(case)?;
    Some(format!(
        "{reason}; candidate skipped; excluded from supported accuracy until native semantics are verified"
    ))
}

fn deferred_default_engine_oracle_gap_reason_text(case: &OpenRulesCase) -> Option<&'static str> {
    [
        ("official_oracle_fixture_gap", "official oracle fixture gap"),
        (
            "defer_empty_non_empty",
            "deferred empty/non_empty oracle semantics",
        ),
        (
            "defer_domain_placeholder_column_ref",
            "deferred domain placeholder column-ref oracle semantics",
        ),
        (
            "defer_not_unique_relationship",
            "deferred not-unique relationship oracle semantics",
        ),
        (
            "defer_duplicate_match_dataset",
            "deferred duplicate match dataset oracle semantics",
        ),
        ("defer_date_operator", "deferred date oracle semantics"),
        ("defer_unique_set", "deferred unique-set oracle semantics"),
        (
            "defer_dy_operation",
            "deferred DY operation oracle semantics",
        ),
        ("defer_sort_operator", "deferred sort oracle semantics"),
        ("defer_etcd_length", "deferred ETCD length oracle semantics"),
        (
            "defer_multi_base_match_dataset",
            "deferred multi-base match dataset oracle semantics",
        ),
        (
            "defer_distinct_operation",
            "deferred distinct operation oracle semantics",
        ),
        (
            "standard_filter_oracle_gap",
            "standard applicability oracle semantics",
        ),
        (
            "required_value_metadata",
            "required value metadata oracle semantics",
        ),
        ("dataset_presence", "dataset presence oracle semantics"),
        ("date_operator", "date oracle semantics"),
        ("domain_presence", "domain presence oracle semantics"),
        ("variable_metadata", "variable metadata oracle semantics"),
        (
            "domain_placeholder_column_ref_comparator",
            "domain placeholder column-ref comparator oracle semantics",
        ),
        ("empty_non_empty", "empty/non_empty oracle semantics"),
        ("missing_column", "missing-column oracle semantics"),
        ("operation", "operation oracle semantics"),
        (
            "scope_wide_reference_distinct",
            "scope-wide reference distinct oracle semantics",
        ),
        (
            "reference_distinct_official_empty",
            "reference distinct official-empty oracle semantics",
        ),
        (
            "reference_distinct_fixture_row",
            "reference distinct fixture row oracle semantics",
        ),
        (
            "reference_distinct_cardinality",
            "reference distinct cardinality oracle semantics",
        ),
        (
            "supported_reference_distinct",
            "reference distinct oracle semantics",
        ),
        ("record_row_locator", "record row locator oracle semantics"),
        (
            "usdm_hand_port_entity_scope",
            "USDM hand-port entity-scope oracle semantics",
        ),
        (
            "record_count_operation",
            "record-count operation oracle semantics",
        ),
        (
            "usdm_join_operation",
            "USDM join operation oracle semantics",
        ),
        ("xhtml_operation", "XHTML operation oracle semantics"),
        (
            "defer_positive_zero_probe",
            "deferred positive-zero probe oracle semantics",
        ),
    ]
    .into_iter()
    .find_map(|(category, reason)| {
        core_api::rule_id_has_oracle_gap_category(&case.rule_id, category).then_some(reason)
    })
}
