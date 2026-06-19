from cdisc_rulekit.classify import classify_rules
from cdisc_rulekit.load_open_rules import load_open_rules
from cdisc_rulekit.load_p21 import load_p21_rules
from cdisc_rulekit.map_rules import map_p21_to_core
from cdisc_rulekit.models import CanonicalRule


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
