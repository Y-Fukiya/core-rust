import pytest

from cdisc_rulekit.errors import CliUsageError
import cdisc_rulekit.load_p21_config as p21_config
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

    with pytest.raises(ValueError, match="source-label values may contain only"):
        load_p21_config_rules([config], source_labels=["/tmp/config.xml"])


def test_load_p21_config_rejects_source_label_separators(tmp_path):
    config = tmp_path / "config.xml"
    config.write_text('<config><rule id="SD0001" /></config>', encoding="utf-8")

    with pytest.raises(ValueError, match="source-label values may contain only"):
        load_p21_config_rules([config], source_labels=["bad|label"])

    with pytest.raises(ValueError, match="source-label values may contain only"):
        load_p21_config_rules([config], source_labels=["bad\nlabel"])


def test_load_p21_config_sanitizes_default_source_label_filename(tmp_path):
    config = tmp_path / "bad|name.xml"
    config.write_text('<config><rule id="SD0001" /></config>', encoding="utf-8")

    rules, warnings = load_p21_config_rules([config])

    assert warnings == []
    assert rules[0].source_path == "source_001:bad_name.xml"


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


def test_load_p21_config_documents_intentional_source_tree_xml_fallback():
    assert p21_config.__doc__ is not None
    policy = " ".join(p21_config.__doc__.split())
    assert "Installed environments should use defusedxml" in policy
    assert "source-tree smoke fallback is intentional" in policy
    assert "DTD/entity preflight" in policy


def test_load_p21_config_wraps_configured_xml_parser_exceptions(tmp_path, monkeypatch):
    config = tmp_path / "blocked.xml"
    config.write_text("<config />", encoding="utf-8")

    class ParserBlocked(Exception):
        pass

    class BlockingParser:
        @staticmethod
        def fromstring(_payload):
            raise ParserBlocked("blocked entity")

    monkeypatch.setattr(p21_config, "DefusedET", BlockingParser)
    monkeypatch.setattr(p21_config, "XML_PARSE_EXCEPTIONS", (ParserBlocked,))

    with pytest.raises(ValueError, match="malformed XML configuration") as excinfo:
        load_p21_config_rules([config])

    assert str(config) in str(excinfo.value)
    assert isinstance(excinfo.value.__cause__, ParserBlocked)


def test_load_p21_config_rejects_non_file_inputs(tmp_path):
    with pytest.raises(ValueError, match="XML configuration input must be a regular file") as excinfo:
        load_p21_config_rules([tmp_path])

    assert str(tmp_path) in str(excinfo.value)


def test_load_p21_config_rejects_missing_inputs(tmp_path):
    missing = tmp_path / "missing.xml"

    with pytest.raises(ValueError, match="XML configuration file does not exist") as excinfo:
        load_p21_config_rules([missing])

    assert str(missing) in str(excinfo.value)


def test_load_p21_config_rejects_oversized_xml_before_reading(tmp_path, monkeypatch):
    config = tmp_path / "large.xml"
    config.write_text("<config />", encoding="utf-8")
    monkeypatch.setattr(p21_config, "MAX_CONFIG_BYTES", 4)
    monkeypatch.setattr(
        p21_config.Path,
        "read_bytes",
        lambda _path: pytest.fail("oversized XML should be rejected before read_bytes"),
    )

    with pytest.raises(ValueError, match="XML configuration exceeds 4 bytes"):
        load_p21_config_rules([config])


def test_load_p21_config_reports_unreadable_xml_as_usage_error(tmp_path, monkeypatch):
    config = tmp_path / "unreadable.xml"
    config.write_text("<config />", encoding="utf-8")

    def raise_permission_error(_path):
        raise PermissionError("permission denied")

    monkeypatch.setattr(p21_config.Path, "read_bytes", raise_permission_error)

    with pytest.raises(CliUsageError, match="could not read XML configuration") as excinfo:
        load_p21_config_rules([config])

    assert str(config) in str(excinfo.value)
    assert isinstance(excinfo.value.__cause__, PermissionError)


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


def test_load_p21_config_trims_attribute_values(tmp_path):
    config = tmp_path / "trim.xml"
    config.write_text(
        '<config agency=" FDA "><standard name=" SDTM-IG " version=" 3.3 "><rule id=" SD0001 " /></standard></config>',
        encoding="utf-8",
    )

    rules, warnings = load_p21_config_rules([config])

    assert warnings == []
    assert rules[0].p21_rule_id == "SD0001"
    assert rules[0].agency == "FDA"
    assert rules[0].standard_name == "SDTM-IG"
    assert rules[0].standard_version == "3.3"


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
