from __future__ import annotations

import re
from dataclasses import asdict, dataclass

from .models import CanonicalRule

MACRO_RE = re.compile(r"(%[^%]*[A-Z_][^%]*%|\{[^}]*[A-Z_][^}]*\}|\$[A-Z0-9_]+|--|\bP\d+\b)", re.I)
DOCUMENTATION_FIELDS = {"message", "description", "all_attributes_json"}


@dataclass(frozen=True)
class MacroFinding:
    source_rule_key: str | None
    p21_rule_id: str | None
    agency: str | None
    standard_name: str | None
    standard_version: str | None
    p21_rule_type: str | None
    field: str
    macro_token: str
    macro_family: str
    convertibility_impact: str

    def to_dict(self) -> dict[str, object]:
        return asdict(self)


def macro_family(token: str) -> str:
    upper = token.upper()
    if re.fullmatch(r"P\d+", upper):
        return "PROPERTY_PARAMETER"
    if "@CLAUSE" in upper:
        return "VALUE_LEVEL_CONFIG_MACRO"
    if upper.startswith("%DOMAIN"):
        return "DOMAIN_PLACEHOLDER"
    if upper.startswith("%SYSTEM."):
        return "SYSTEM_METADATA_MACRO"
    if upper.startswith("%VARIABLES.CONFIG") or upper.startswith("%VARIABLE.CONFIG"):
        return "VARIABLE_CONFIG_MACRO"
    if upper.startswith("%VARIABLES$") or "$CONFIG" in upper:
        return "VARIABLE_CONFIG_MACRO"
    if upper.startswith("%VARIABLES"):
        return "VARIABLE_SELECTOR_MACRO"
    if upper == "%VARIABLE%" or upper.startswith("%VARIABLE."):
        return "VARIABLE_MESSAGE_PLACEHOLDER"
    if upper.startswith("%") and upper.endswith("]%"):
        return "NESTED_SELECTOR_FRAGMENT"
    if upper == "--":
        return "DATASET_VARIABLE_MACRO"
    if upper.startswith("$"):
        return "DOLLAR_MACRO"
    if upper.startswith("{"):
        return "BRACE_TEMPLATE"
    if upper.startswith("%"):
        return "PERCENT_MACRO"
    return "UNKNOWN_MACRO"


def _impact(field: str, family: str) -> str:
    if field in DOCUMENTATION_FIELDS:
        return "DOCUMENTATION_ONLY"
    if family == "VARIABLE_MESSAGE_PLACEHOLDER" and field in {"message", "description"}:
        return "DOCUMENTATION_ONLY"
    return "BLOCKS_AUTOMATION"


def _scan_values(rule: CanonicalRule) -> list[tuple[str, str]]:
    values: list[tuple[str, str]] = []
    for field, value in sorted(rule.raw_condition.items()):
        if value not in (None, ""):
            values.append((field, str(value)))
    for variable in rule.variables:
        values.append(("variable", variable))
    if rule.target:
        values.append(("target", rule.target))
    if rule.message:
        values.append(("message", rule.message))
    if rule.description:
        values.append(("description", rule.description))
    return values


def macro_findings_for_rule(rule: CanonicalRule) -> list[MacroFinding]:
    findings: list[MacroFinding] = []
    seen: set[tuple[str, str]] = set()
    for field, value in _scan_values(rule):
        for match in MACRO_RE.finditer(value):
            token = match.group(0)
            key = (field, token)
            if key in seen:
                continue
            seen.add(key)
            family = macro_family(token)
            findings.append(
                MacroFinding(
                    source_rule_key=rule.source_rule_key,
                    p21_rule_id=rule.p21_rule_id,
                    agency=rule.agency,
                    standard_name=rule.standard_name,
                    standard_version=rule.standard_version,
                    p21_rule_type=rule.p21_rule_type,
                    field=field,
                    macro_token=token,
                    macro_family=family,
                    convertibility_impact=_impact(field, family),
                ),
            )
    return findings


def structural_blocking_macro_families(rule: CanonicalRule) -> list[str]:
    families = {
        finding.macro_family
        for finding in macro_findings_for_rule(rule)
        if finding.convertibility_impact == "BLOCKS_AUTOMATION"
    }
    return sorted(families)
