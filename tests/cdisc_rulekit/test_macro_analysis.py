from cdisc_rulekit.macro_analysis import (
    macro_findings_for_rule,
    structural_blocking_macro_families,
)
from cdisc_rulekit.models import CanonicalRule


def test_macro_analysis_separates_structural_macros_from_documentation_copies():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="CT2002",
        source_rule_key="FDA|SDTMIG|3.3|CT2002",
        p21_rule_id="CT2002",
        agency="FDA",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Match",
        message="%Variable% value not found in codelist",
        variables=["%Variables.Config.CodeList.Extensible:Y%"],
        raw_condition={
            "variable": "%Variables.Config.CodeList.Extensible:Y%",
            "all_attributes_json": '{"Message":"%Variable% copied here"}',
        },
    )

    findings = macro_findings_for_rule(rule)

    assert {finding.macro_family for finding in findings} >= {
        "VARIABLE_CONFIG_MACRO",
        "VARIABLE_MESSAGE_PLACEHOLDER",
    }
    documentation = [finding for finding in findings if finding.field == "all_attributes_json"]
    assert documentation
    assert {finding.convertibility_impact for finding in documentation} == {"DOCUMENTATION_ONLY"}
    assert structural_blocking_macro_families(rule) == ["VARIABLE_CONFIG_MACRO"]


def test_macro_analysis_detects_p21_property_parameters():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0001",
        source_rule_key="FDA|SDTMIG|3.3|SD0001",
        p21_rule_id="SD0001",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Property",
        raw_condition={"test": "P1 @gt 0 @or P2 != ''"},
    )

    findings = macro_findings_for_rule(rule)

    assert [finding.macro_token for finding in findings] == ["P1", "P2"]
    assert {finding.macro_family for finding in findings} == {"PROPERTY_PARAMETER"}
    assert structural_blocking_macro_families(rule) == ["PROPERTY_PARAMETER"]


def test_macro_analysis_classifies_common_percent_macro_families():
    rule = CanonicalRule(
        source="P21",
        source_rule_id="MIXED",
        p21_rule_id="MIXED",
        raw_condition={
            "test": "%Domain% == 'AE' @and %System.MedDRA.Version% != ''",
            "variable": "%Variables[%Domain%/%DECOD]%",
        },
    )

    findings = macro_findings_for_rule(rule)

    families = {finding.macro_family for finding in findings}
    assert "DOMAIN_PLACEHOLDER" in families
    assert "SYSTEM_METADATA_MACRO" in families
    assert "VARIABLE_SELECTOR_MACRO" in families
    assert structural_blocking_macro_families(rule) == [
        "DOMAIN_PLACEHOLDER",
        "NESTED_SELECTOR_FRAGMENT",
        "SYSTEM_METADATA_MACRO",
        "VARIABLE_SELECTOR_MACRO",
    ]
