from cdisc_rulekit.load_open_rules import load_open_rules
from cdisc_rulekit.load_p21 import load_p21_rules
from cdisc_rulekit.map_rules import map_p21_to_core
from cdisc_rulekit.models import CanonicalRule


def test_cg_id_mapping_has_high_confidence(p21_rules_path, p21_domain_map_path, open_rules_repo_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)

    mappings = map_p21_to_core(p21_rules, core_rules)

    mapping = next(item for item in mappings if item.p21_rule_id == "SD0001")
    assert mapping.core_rule_id == "CORE-000001"
    assert mapping.match_type == "CG_ID"
    assert mapping.confidence >= 0.90
    assert mapping.cdisc_rule_id_overlap == ["CG0001"]


def test_fuzzy_mapping_stays_fuzzy(open_rules_repo_path):
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)
    p21_rule = CanonicalRule(
        source="P21",
        source_rule_id="FUZZY1",
        p21_rule_id="FUZZY1",
        standard_name="SDTM-IG",
        p21_rule_type="Match",
        domains=["AE"],
        variables=["AETERM"],
        message="AETERM must be present.",
        description="AETERM must be populated.",
    )

    mapping = map_p21_to_core([p21_rule], core_rules)[0]

    assert mapping.core_rule_id == "CORE-000001"
    assert mapping.match_type == "FUZZY"
    assert mapping.confidence >= 0.60


def test_unrelated_rule_gets_none_mapping(open_rules_repo_path):
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)
    p21_rule = CanonicalRule(
        source="P21",
        source_rule_id="NONE1",
        p21_rule_id="NONE1",
        standard_name="SEND-IG",
        p21_rule_type="Lookup",
        domains=["TX"],
        variables=["TXVAL"],
        message="Unrelated message",
    )

    mapping = map_p21_to_core([p21_rule], core_rules)[0]

    assert mapping.core_rule_id is None
    assert mapping.match_type == "NONE"
    assert mapping.confidence == 0


def test_mapping_returns_one_row_per_p21_rule(p21_rules_path, p21_domain_map_path, open_rules_repo_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)
    core_rules, _inventory, _warnings = load_open_rules(open_rules_repo_path)

    mappings = map_p21_to_core(p21_rules, core_rules)

    assert len(mappings) == len(p21_rules)
