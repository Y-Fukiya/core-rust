from cdisc_rulekit.io_utils import (
    normalize_blank,
    read_jsonl,
    split_semicolon_list,
    write_jsonl,
)
from cdisc_rulekit.models import CanonicalRule, OperatorInventoryItem, RuleMapping


def test_blank_and_semicolon_normalization():
    assert normalize_blank("") is None
    assert normalize_blank(" nan ") is None
    assert normalize_blank(None) is None
    assert split_semicolon_list(" AE ; CM;AE ") == ["AE", "CM"]


def test_canonical_rule_serializes_lists_and_dicts():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0001",
        p21_rule_id="SD0001",
        cdisc_rule_ids=["CG0001"],
        domains=["AE"],
        variables=["AETERM"],
        raw_condition={"terms": ["HEADACHE"]},
    )

    serialized = rule.to_dict()

    assert serialized["source"] == "P21"
    assert serialized["cdisc_rule_ids"] == ["CG0001"]
    assert serialized["domains"] == ["AE"]
    assert serialized["raw_condition"] == {"terms": ["HEADACHE"]}


def test_mapping_and_operator_inventory_serialize():
    mapping = RuleMapping(
        p21_rule_id="SD0001",
        core_rule_id="CORE-000001",
        match_type="CG_ID",
        confidence=0.95,
        cdisc_rule_id_overlap=["CG0001"],
    )
    inventory_item = OperatorInventoryItem(
        core_rule_id="CORE-000001",
        source_path="Published/CORE-000001/rule.yml",
        operator="all",
        path="Check.all",
        node_kind="dict",
        name_values=["AETERM"],
        raw_keys=["all"],
    )

    assert mapping.to_dict()["match_type"] == "CG_ID"
    assert inventory_item.to_dict()["operator"] == "all"


def test_jsonl_round_trip(tmp_path):
    path = tmp_path / "rows.jsonl"
    rows = [{"id": "one", "values": ["A"]}, {"id": "two", "values": ["B"]}]

    write_jsonl(path, rows)

    assert read_jsonl(path) == rows
