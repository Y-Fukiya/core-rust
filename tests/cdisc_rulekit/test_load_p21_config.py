import pytest

from cdisc_rulekit.load_p21_config import load_p21_config_rules


def test_load_p21_config_accepts_multiple_local_xml_files(tmp_path):
    first = tmp_path / "first.xml"
    second = tmp_path / "second.xml"
    first.write_text('<config><rule id="SD0001"><message>First CG0001</message></rule></config>', encoding="utf-8")
    second.write_text('<config><rule id="SD0002"><message>Second CG0002</message></rule></config>', encoding="utf-8")

    rules, warnings = load_p21_config_rules([first, second])

    assert warnings == []
    assert [rule.p21_rule_id for rule in rules] == ["SD0001", "SD0002"]
    assert [rule.cdisc_rule_ids for rule in rules] == [["CG0001"], ["CG0002"]]
    assert {rule.source_path for rule in rules} == {"source_001:first.xml", "source_002:second.xml"}


def test_load_p21_config_accepts_stable_source_labels(tmp_path):
    first = tmp_path / "first.xml"
    second = tmp_path / "second.xml"
    first.write_text('<config><rule id="SD0001" /></config>', encoding="utf-8")
    second.write_text('<config><rule id="SD0002" /></config>', encoding="utf-8")

    rules, warnings = load_p21_config_rules([first, second], source_labels=["sdtm33", "sdtm34"])

    assert warnings == []
    assert [rule.source_path for rule in rules] == ["sdtm33", "sdtm34"]
    assert [rule.source_rule_key.rsplit("|", 1)[-1] for rule in rules] == ["sdtm33", "sdtm34"]


def test_load_p21_config_rejects_path_like_source_labels(tmp_path):
    config = tmp_path / "config.xml"
    config.write_text('<config><rule id="SD0001" /></config>', encoding="utf-8")

    with pytest.raises(ValueError, match="source-label values must be labels"):
        load_p21_config_rules([config], source_labels=["/tmp/config.xml"])


def test_load_p21_config_rejects_mismatched_source_label_count(tmp_path):
    config = tmp_path / "config.xml"
    config.write_text('<config><rule id="SD0001" /></config>', encoding="utf-8")

    with pytest.raises(ValueError, match="source-label count must match"):
        load_p21_config_rules([config], source_labels=["one", "two"])


def test_load_p21_config_warns_when_no_rule_elements_are_found(tmp_path):
    config = tmp_path / "empty.xml"
    config.write_text("<config><standard name=\"SDTM-IG\" /></config>", encoding="utf-8")

    rules, warnings = load_p21_config_rules([config])

    assert rules == []
    assert warnings == ["source_001:empty.xml: no rule elements found"]


def test_load_p21_config_rejects_dtd_or_entity_declarations(tmp_path):
    config = tmp_path / "dangerous.xml"
    config.write_text(
        '<!DOCTYPE config [<!ENTITY local SYSTEM "file:///etc/passwd">]><config>&local;</config>',
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="DTD/entity declarations are not supported"):
        load_p21_config_rules([config])


def test_load_p21_config_skips_rules_without_rule_ids(tmp_path):
    config = tmp_path / "missing-id.xml"
    config.write_text(
        '<config><rule><message>Missing id</message></rule><rule id="SD0001" /></config>',
        encoding="utf-8",
    )

    rules, warnings = load_p21_config_rules([config])

    assert [rule.p21_rule_id for rule in rules] == ["SD0001"]
    assert warnings == ["source_001:missing-id.xml: rule element without id was skipped"]


def test_load_p21_config_reads_nested_domain_class_and_variable_lists(tmp_path):
    config = tmp_path / "nested.xml"
    config.write_text(
        """
<config name="SDTM-IG">
  <standard name="SDTM-IG" version="3.4">
    <rule id="SD1234">
      <domains><domain>AE</domain><domain>CM</domain></domains>
      <classes><class>EVENTS</class><class>INTERVENTIONS</class></classes>
      <variables><variable>AETERM</variable><variable>AEDECOD</variable></variables>
      <check id="nested-child">not a rule</check>
    </rule>
  </standard>
</config>
""".strip(),
        encoding="utf-8",
    )

    rules, warnings = load_p21_config_rules([config])

    assert warnings == []
    assert len(rules) == 1
    assert rules[0].domains == ["AE", "CM"]
    assert rules[0].classes == ["EVENTS", "INTERVENTIONS"]
    assert rules[0].variables == ["AEDECOD", "AETERM"]


def test_load_p21_config_reads_list_wrapper_aliases(tmp_path):
    config = tmp_path / "aliases.xml"
    config.write_text(
        """
<config>
  <rule id="SD1234">
    <domainList><item>ae</item><value>cm</value></domainList>
    <classList><item>events</item></classList>
    <variableList><item>aeterm</item><value>aedecod</value></variableList>
  </rule>
</config>
""".strip(),
        encoding="utf-8",
    )

    rules, warnings = load_p21_config_rules([config])

    assert warnings == []
    assert len(rules) == 1
    assert rules[0].domains == ["AE", "CM"]
    assert rules[0].classes == ["EVENTS"]
    assert rules[0].variables == ["AEDECOD", "AETERM"]
