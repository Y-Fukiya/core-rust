from cdisc_rulekit.load_open_rules import load_open_rules


def test_load_open_rules_scans_published_by_default(open_rules_repo_path):
    rules, testdata_inventory, warnings = load_open_rules(open_rules_repo_path)

    assert warnings == []
    assert [rule.core_rule_id for rule in rules] == ["CORE-000001"]
    assert testdata_inventory


def test_load_open_rules_can_include_unpublished(open_rules_repo_path):
    rules, _testdata_inventory, _warnings = load_open_rules(
        open_rules_repo_path,
        include_unpublished=True,
    )

    assert [rule.core_rule_id for rule in rules] == ["CORE-000001", "CORE-DRAFT-0001"]


def test_load_open_rules_extracts_core_fields(open_rules_repo_path):
    rules, _testdata_inventory, _warnings = load_open_rules(open_rules_repo_path)

    rule = rules[0]
    assert rule.source == "CDISC_OPEN_RULES"
    assert rule.source_rule_id == "CORE-000001"
    assert rule.core_rule_id == "CORE-000001"
    assert rule.cdisc_rule_ids == ["CG0001"]
    assert rule.standard_name == "SDTMIG"
    assert rule.standard_version == "3.4"
    assert rule.substandard == "SDTMIG"
    assert rule.domains == ["AE"]
    assert rule.classes == ["EVENTS"]
    assert "AETERM" in rule.variables
    assert rule.message == "AETERM must be present."
    assert rule.core_rule_type == "Record Data"


def test_load_open_rules_inventories_existing_test_data(open_rules_repo_path):
    _rules, testdata_inventory, _warnings = load_open_rules(open_rules_repo_path)

    assert {
        "scope": "Published",
        "core_rule_id": "CORE-000001",
        "case_type": "positive",
        "case_id": "01",
        "data_file": "ae.csv",
    } in testdata_inventory
