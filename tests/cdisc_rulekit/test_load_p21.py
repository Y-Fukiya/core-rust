from cdisc_rulekit.load_p21 import extract_cg_ids, load_p21_rules


def test_extract_cg_ids_deduplicates_and_sorts():
    assert extract_cg_ids("CG0002 and CG0001", "CG0001") == ["CG0001", "CG0002"]


def test_load_p21_rules_normalizes_fixture_rows(p21_rules_path, p21_domain_map_path):
    rules, warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    assert warnings == []
    assert len(rules) == 4

    first = next(rule for rule in rules if rule.p21_rule_id == "SD0001")
    assert first.source == "P21"
    assert first.source_rule_id == "SD0001"
    assert first.cdisc_rule_ids == ["CG0001"]
    assert first.domains == ["AE"]
    assert first.classes == ["EVENTS"]
    assert first.variables == ["AETERM"]
    assert first.raw_condition["terms"] == "HEADACHE;NAUSEA"
    assert first.raw_record["rule_id"] == "SD0001"


def test_load_p21_rules_normalizes_blank_and_nan_values(p21_rules_path, p21_domain_map_path):
    rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    regex_rule = next(rule for rule in rules if rule.p21_rule_id == "SD0002")
    assert regex_rule.cdisc_rule_ids == []
    assert regex_rule.raw_record["publisher_id_raw"] is None
    assert regex_rule.raw_record["all_attributes_json"] is None


def test_load_p21_rules_excludes_inactive_domain_mappings(p21_rules_path, p21_domain_map_path):
    rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    first = next(rule for rule in rules if rule.p21_rule_id == "SD0001")
    assert first.domains == ["AE"]
    assert "CM" not in first.domains


def test_load_p21_rules_preserves_define_xml_row(p21_rules_path, p21_domain_map_path):
    rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)

    define_rule = next(rule for rule in rules if rule.p21_rule_id == "DEF001")
    assert define_rule.standard_name == "Define.xml"
    assert define_rule.p21_rule_type == "Schematron"
    assert define_rule.source_path == "Define.xml"
    assert define_rule.domains == ["GLOBAL"]


def test_load_p21_rules_accepts_utf8_bom(tmp_path, p21_rules_path, p21_domain_map_path):
    bom_rules = tmp_path / "rules_with_bom.csv"
    bom_rules.write_text("\ufeff" + p21_rules_path.read_text(encoding="utf-8"), encoding="utf-8")

    rules, warnings = load_p21_rules(bom_rules, p21_domain_map_path)

    assert warnings == []
    first = next(rule for rule in rules if rule.p21_rule_id == "SD0001")
    assert first.raw_record["config_version"] == "2204.0"


def test_load_p21_rules_assigns_unique_source_rule_key_for_duplicate_rule_ids(tmp_path):
    rules_path = tmp_path / "rules.csv"
    rules_path.write_text(
        "\n".join(
            [
                "config_version,agency,config_name,standard_name,standard_version,rule_id,rule_type,message,description,target,variable,domains,domain_classes,source_xml_path",
                "2204.0,FDA,SDTMIG,SDTM-IG,3.2,SD0001,Required,Message,Description,USUBJID,USUBJID,DM,SPECIAL,sdtmig-3.2.xml",
                "2204.0,FDA,SDTMIG,SDTM-IG,3.3,SD0001,Required,Message,Description,USUBJID,USUBJID,DM,SPECIAL,sdtmig-3.3.xml",
                "",
            ],
        ),
        encoding="utf-8",
    )

    rules, warnings = load_p21_rules(rules_path)

    assert warnings == []
    assert [rule.p21_rule_id for rule in rules] == ["SD0001", "SD0001"]
    assert len({rule.source_rule_key for rule in rules}) == 2
    assert rules[0].source_rule_key.endswith("SD0001|sdtmig-3.2.xml")
