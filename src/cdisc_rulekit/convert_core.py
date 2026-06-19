from __future__ import annotations

from typing import Any

from .io_utils import split_semicolon_list
from .map_rules import standard_key
from .models import CanonicalRule


def generated_rule_id(rule: CanonicalRule) -> str:
    standard = standard_key(rule.standard_name) or "UNKNOWN"
    source_id = rule.p21_rule_id or rule.source_rule_id
    return f"P21PORT-{standard}-{source_id}"


def primary_domain(rule: CanonicalRule) -> str:
    for domain in rule.domains:
        if domain.upper() != "GLOBAL":
            return domain.upper()
    return "DM"


def primary_variable(rule: CanonicalRule) -> str:
    if rule.target:
        return rule.target.upper()
    if rule.variables:
        return rule.variables[0].upper()
    return "VALUE"


def _reference_id(rule: CanonicalRule) -> str:
    if rule.cdisc_rule_ids:
        return rule.cdisc_rule_ids[0]
    return f"P21:{rule.p21_rule_id or rule.source_rule_id}"


def _check(rule: CanonicalRule) -> dict[str, Any]:
    variable = primary_variable(rule)
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type == "REGEX":
        pattern = rule.raw_condition.get("test") or rule.raw_condition.get("terms") or ".*"
        return {"not_matches": {"name": variable, "value": str(pattern)}}
    if rule_type == "MATCH":
        terms = split_semicolon_list(rule.raw_condition.get("terms"))
        return {"not_in": {"name": variable, "values": terms}}
    if rule_type == "CONDITION":
        return {"condition": {"name": variable, "source": rule.raw_condition}}
    if rule_type == "REQUIRED":
        return {"is_empty": {"name": variable}}
    if rule_type == "FIND":
        return {"not_exists": {"name": variable}}
    return {"manual_review": {"name": variable}}


def build_rule_yml(rule: CanonicalRule) -> dict[str, Any]:
    domain = primary_domain(rule)
    return {
        "Authorities": [
            {
                "Organization": "CDISC",
                "Standards": [
                    {
                        "Name": rule.standard_name,
                        "Version": str(rule.standard_version or ""),
                        "References": [
                            {
                                "Origin": "P21 config extract",
                                "Rule Identifier": {
                                    "Id": _reference_id(rule),
                                    "Version": "1",
                                },
                            }
                        ],
                    }
                ],
            }
        ],
        "Check": _check(rule),
        "Core": {
            "Id": generated_rule_id(rule),
            "Status": "Draft",
            "Version": "0.1",
            "Description": rule.description or rule.message or generated_rule_id(rule),
        },
        "Executability": "Partially Executable",
        "Outcome": {"Message": rule.message or f"{primary_variable(rule)} violates {rule.p21_rule_id}"},
        "Rule Type": "Record Data",
        "Scope": {
            "Classes": {"Include": rule.classes},
            "Domains": {"Include": [domain]},
        },
        "Sensitivity": "Record",
    }
