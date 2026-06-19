from cdisc_rulekit.load_open_rules import load_open_rules
from cdisc_rulekit.operator_inventory import build_operator_inventory


def test_operator_inventory_records_check_shapes(open_rules_repo_path):
    rules, _testdata_inventory, _warnings = load_open_rules(open_rules_repo_path)

    inventory = build_operator_inventory(rules)

    assert inventory
    assert any(item.core_rule_id == "CORE-000001" for item in inventory)
    assert any(item.operator == "all" for item in inventory)
    assert any(item.operator == "not_empty" for item in inventory)
    assert any(item.raw_keys == ["all"] for item in inventory)
