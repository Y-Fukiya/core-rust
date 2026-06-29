from cdisc_rulekit.classify import classify_rules
from cdisc_rulekit.load_open_rules import load_open_rules
from cdisc_rulekit.load_p21 import load_p21_rules
from cdisc_rulekit.map_rules import map_p21_to_core
from cdisc_rulekit.models import CanonicalRule, RuleMapping


def test_cg_id_mapping_classifies_as_native_core(
    p21_rules_path,
    p21_domain_map_path,
    open_rules_repo_path,
):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)
    mappings = map_p21_to_core(p21_rules, core_rules)

    classified = classify_rules(p21_rules, mappings)

    native = next(rule for rule in classified if rule.p21_rule_id == "SD0001")
    assert native.conversion_status == "NATIVE_CORE"
    assert native.core_rule_id == "CORE-000001"
    assert "HAS_NATIVE_CORE_MAPPING" in native.conversion_reasons


def test_simple_regex_classifies_as_auto_convertible(p21_rules_path, p21_domain_map_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    classified = classify_rules(p21_rules, [])

    regex_rule = next(rule for rule in classified if rule.p21_rule_id == "SD0002")
    assert regex_rule.conversion_status == "AUTO_CONVERTIBLE"
    assert "SIMPLE_REGEX" in regex_rule.conversion_reasons


def test_regex_with_unsupported_rust_syntax_requires_manual_review():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDREGEX-LOOKAHEAD",
        p21_rule_id="SDREGEX-LOOKAHEAD",
        standard_name="SDTM-IG",
        p21_rule_type="Regex",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
        raw_condition={"test": r"^(?=.*SUBJ)\w+$"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "MANUAL_REQUIRED"
    assert "UNSUPPORTED_RUST_REGEX_SYNTAX" in classified[0].conversion_reasons


def test_simple_required_classifies_with_required_reason():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDREQ1",
        p21_rule_id="SDREQ1",
        standard_name="SDTM-IG",
        p21_rule_type="Required",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "SIMPLE_REQUIRED_CHECK" in classified[0].conversion_reasons


def test_define_xml_and_schematron_classify_as_manual(p21_rules_path, p21_domain_map_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    classified = classify_rules(p21_rules, [])

    define_rule = next(rule for rule in classified if rule.p21_rule_id == "DEF001")
    assert define_rule.conversion_status == "MANUAL_REQUIRED"
    assert {
        "DEFINE_XML_RULE",
        "SCHEMATRON_RULE",
    }.intersection(define_rule.conversion_reasons)


def test_malformed_row_classifies_as_unsupported():
    malformed = CanonicalRule(source="P21", source_rule_id="")

    classified = classify_rules([malformed], [])

    assert classified[0].conversion_status == "UNSUPPORTED"
    assert "MALFORMED_INPUT" in classified[0].conversion_reasons


def test_fuzzy_mapping_never_classifies_as_native_core(open_rules_repo_path):
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)
    fuzzy_rule = CanonicalRule(
        source="P21",
        source_rule_id="FUZZY1",
        p21_rule_id="FUZZY1",
        standard_name="SDTM-IG",
        p21_rule_type="Match",
        domains=["AE"],
        variables=["AETERM"],
        target="AETERM",
        message="AETERM must be present.",
        description="AETERM must be populated.",
    )
    mappings = map_p21_to_core([fuzzy_rule], core_rules)

    classified = classify_rules([fuzzy_rule], mappings)

    assert mappings[0].match_type == "FUZZY"
    assert classified[0].conversion_status != "NATIVE_CORE"
    assert "FUZZY_CORE_CANDIDATE" in classified[0].conversion_reasons


def test_general_all_domain_rule_is_not_cross_dataset_only_because_suppqual_is_in_scope():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="CT2002",
        p21_rule_id="CT2002",
        standard_name="SDTM-IG",
        p21_rule_type="Match",
        category="Terminology",
        domains=["AE", "DM", "RELREC", "SUPPQUAL"],
        variables=["%VARIABLES.CONFIG.CODELIST.EXTENSIBLE:Y%"],
        message="%Variable% value not found in extensible codelist",
        description="Variable should be populated with terms from its CDISC controlled terminology codelist.",
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "MANUAL_REQUIRED"
    assert "UNRESOLVED_VARIABLE_MACRO" in classified[0].conversion_reasons
    assert "CROSS_DATASET_DEPENDENCY" not in classified[0].conversion_reasons


def test_suppqual_only_rule_classifies_as_cross_dataset_manual():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SUPP001",
        p21_rule_id="SUPP001",
        standard_name="SDTM-IG",
        p21_rule_type="Condition",
        domains=["SUPPQUAL"],
        variables=["QNAM"],
        target="QNAM",
        message="SUPPQUAL QNAM must be consistent with parent domain",
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "MANUAL_REQUIRED"
    assert "CROSS_DATASET_DEPENDENCY" in classified[0].conversion_reasons


def test_condition_with_inferable_test_target_classifies_as_auto_convertible():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1035",
        p21_rule_id="SD1035",
        standard_name="SDTM-IG",
        p21_rule_type="Condition",
        domains=["DS"],
        raw_condition={"test": "DSCAT !=''"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "INFERRED_CONDITION_TARGET" in classified[0].conversion_reasons


def test_condition_column_comparator_classifies_as_auto_convertible():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1315",
        p21_rule_id="SD1315",
        standard_name="SDTM-IG",
        p21_rule_type="Condition",
        domains=["DS"],
        raw_condition={"test": "DSDECOD == DSTERM"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "INFERRED_CONDITION_TARGET" in classified[0].conversion_reasons


def test_condition_with_simple_or_expected_test_classifies_as_auto_convertible():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0089",
        p21_rule_id="SD0089",
        standard_name="SDTM-IG",
        p21_rule_type="Condition",
        domains=["TE"],
        raw_condition={"test": "TEENRL != '' @or TEDUR != ''"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "INFERRED_CONDITION_TARGET" in classified[0].conversion_reasons
    assert "SIMPLE_LOGICAL_CONDITION" in classified[0].conversion_reasons


def test_unique_rule_with_group_by_classifies_as_auto_convertible():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0083",
        p21_rule_id="SD0083",
        standard_name="SDTM-IG",
        p21_rule_type="Unique",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
        raw_condition={"group_by": "STUDYID", "when": "USUBJID != ''"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "SIMPLE_UNIQUE_SET" in classified[0].conversion_reasons


def test_unique_rule_without_group_by_uses_dataset_scope():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1214",
        p21_rule_id="SD1214",
        standard_name="SDTM-IG",
        p21_rule_type="Unique",
        domains=["TS"],
        variables=["TSPARMCD"],
        target="TSPARMCD",
        raw_condition={"when": "TSPARMCD == 'ADDON'"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "SIMPLE_UNIQUE_SET" in classified[0].conversion_reasons


def test_condition_column_equality_classifies_as_auto_convertible():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0085",
        p21_rule_id="SD0085",
        standard_name="SDTM-IG",
        p21_rule_type="Condition",
        domains=["IE"],
        raw_condition={"when": "IEORRES != '' @and IESTRESC != ''", "test": "IEORRES == IESTRESC"},
    )

    classified = classify_rules([rule], [])

    assert classified[0].conversion_status == "AUTO_CONVERTIBLE"
    assert "INFERRED_CONDITION_TARGET" in classified[0].conversion_reasons


def test_classification_uses_source_rule_key_when_rule_ids_are_duplicated():
    first = CanonicalRule(
        source="P21",
        source_rule_id="SD0001",
        source_rule_key="FDA|SDTMIG|3.2|SD0001",
        p21_rule_id="SD0001",
        standard_name="SDTM-IG",
        p21_rule_type="Required",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
    )
    second = first.with_updates(source_rule_key="FDA|SDTMIG|3.3|SD0001", standard_version="3.3")
    mappings = [
        RuleMapping(
            p21_rule_id="SD0001",
            p21_rule_key="FDA|SDTMIG|3.2|SD0001",
            core_rule_id="CORE-OLD",
            match_type="CG_ID",
            confidence=0.95,
        ),
        RuleMapping(
            p21_rule_id="SD0001",
            p21_rule_key="FDA|SDTMIG|3.3|SD0001",
            core_rule_id=None,
            match_type="NONE",
            confidence=0.0,
        ),
    ]

    classified = classify_rules([first, second], mappings)

    assert [rule.core_rule_id for rule in classified] == ["CORE-OLD", None]
    assert [rule.conversion_status for rule in classified] == ["NATIVE_CORE", "AUTO_CONVERTIBLE"]
