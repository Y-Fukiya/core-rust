from __future__ import annotations

import re

from .map_rules import p21_mapping_key, standard_key
from .macro_analysis import structural_blocking_macro_families
from .models import CanonicalRule, RuleMapping
from .p21_condition import infer_condition_target

AUTO_RULE_TYPES = {"MATCH", "REGEX", "CONDITION", "REQUIRED", "FIND"}
SUPPORTED_STANDARDS = {"SDTMIG", "ADAMIG", "SENDIG"}


def _text_contains(rule: CanonicalRule, *needles: str) -> bool:
    haystack = " ".join(
        str(value or "")
        for value in (
            rule.standard_name,
            rule.p21_rule_type,
            rule.category,
            rule.message,
            rule.description,
            rule.source_path,
            " ".join(rule.domains),
            " ".join(rule.variables),
            str(rule.raw_condition),
        )
    ).upper()
    return any(needle.upper() in haystack for needle in needles)


def _raw_has_value(rule: CanonicalRule, *fields: str) -> bool:
    return any(rule.raw_condition.get(field) not in (None, "") for field in fields)


def _has_concrete_domain(rule: CanonicalRule) -> bool:
    return any(domain and domain.upper() != "GLOBAL" for domain in rule.domains)


def _special_cross_dataset_domain(domain: str) -> bool:
    normalized = domain.upper()
    return normalized == "RELREC" or normalized.startswith("SUPP")


def _has_cross_dataset_dependency(rule: CanonicalRule) -> bool:
    domains = {domain.upper() for domain in rule.domains if domain}
    if domains and all(_special_cross_dataset_domain(domain) for domain in domains):
        return True

    fields = [
        rule.target,
        rule.source_path,
        rule.raw_condition.get("target"),
        rule.raw_condition.get("from"),
        rule.raw_condition.get("search"),
        rule.raw_condition.get("where"),
        rule.raw_condition.get("matching"),
    ]
    fields.extend(rule.variables)
    haystack = " ".join(str(value or "") for value in fields).upper()
    return bool(re.search(r"\bRELREC\b|\bSUPP[A-Z0-9_]*\b", haystack))


def _inferred_condition_target(rule: CanonicalRule) -> str | None:
    if (rule.p21_rule_type or "").upper() != "CONDITION":
        return None
    return infer_condition_target(rule.raw_condition.get("test"))


def _has_target_variable(rule: CanonicalRule) -> bool:
    return bool(rule.target or rule.variables or _inferred_condition_target(rule))


def _simple_reason(rule_type: str) -> str:
    if rule_type == "MATCH":
        return "SIMPLE_MATCH_TERMS"
    if rule_type == "REGEX":
        return "SIMPLE_REGEX"
    if rule_type == "CONDITION":
        return "SIMPLE_SAME_RECORD_CONDITION"
    if rule_type == "FIND":
        return "DATASET_PRESENCE_CHECK"
    return "NO_CORE_MAPPING"


def _mapping_by_rule_key(mappings: list[RuleMapping]) -> dict[str, RuleMapping]:
    return {mapping.p21_rule_key or mapping.p21_rule_id: mapping for mapping in mappings}


def _classify(rule: CanonicalRule, mapping: RuleMapping | None) -> CanonicalRule:
    reasons: list[str] = []
    confidence = 0.0
    core_rule_id = mapping.core_rule_id if mapping else rule.core_rule_id

    if not rule.p21_rule_id and not rule.source_rule_id:
        return rule.with_updates(
            conversion_status="UNSUPPORTED",
            conversion_reasons=["MALFORMED_INPUT"],
            conversion_confidence=0.0,
        )

    if mapping and mapping.match_type == "CG_ID" and mapping.confidence >= 0.90 and mapping.core_rule_id:
        return rule.with_updates(
            core_rule_id=mapping.core_rule_id,
            conversion_status="NATIVE_CORE",
            conversion_reasons=["HAS_NATIVE_CORE_MAPPING"],
            conversion_confidence=mapping.confidence,
        )

    if mapping and mapping.match_type == "FUZZY":
        reasons.append("FUZZY_CORE_CANDIDATE")
        confidence = mapping.confidence
    else:
        reasons.append("NO_CORE_MAPPING")

    if _text_contains(rule, "DEFINE.XML"):
        reasons.append("DEFINE_XML_RULE")
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    if _text_contains(rule, "SCHEMATRON"):
        reasons.append("SCHEMATRON_RULE")
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    if _text_contains(rule, "METADATA", "VARORDER", "VARLENGTH"):
        reasons.append("METADATA_RULE")
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    if _has_cross_dataset_dependency(rule):
        reasons.append("CROSS_DATASET_DEPENDENCY")
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    if _raw_has_value(rule, "search", "from"):
        reasons.append("EXTERNAL_LOOKUP_DEPENDENCY")
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    macro_families = structural_blocking_macro_families(rule)
    if macro_families:
        reasons.append("UNRESOLVED_VARIABLE_MACRO")
        reasons.extend(f"P21_MACRO_{family}" for family in macro_families)
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="MANUAL_REQUIRED",
            conversion_reasons=reasons,
            conversion_confidence=confidence,
        )

    rule_type = (rule.p21_rule_type or "").upper()
    standard = standard_key(rule.standard_name)
    if rule_type in AUTO_RULE_TYPES and standard in SUPPORTED_STANDARDS:
        if not _has_concrete_domain(rule):
            reasons.append("NO_CONCRETE_DOMAIN")
            return rule.with_updates(
                core_rule_id=core_rule_id,
                conversion_status="SKELETON_ONLY",
                conversion_reasons=reasons,
                conversion_confidence=confidence,
            )
        if not _has_target_variable(rule):
            reasons.append("NO_TARGET_VARIABLE")
            return rule.with_updates(
                core_rule_id=core_rule_id,
                conversion_status="SKELETON_ONLY",
                conversion_reasons=reasons,
                conversion_confidence=confidence,
            )
        if _inferred_condition_target(rule):
            reasons.append("INFERRED_CONDITION_TARGET")
        reasons.append(_simple_reason(rule_type))
        return rule.with_updates(
            core_rule_id=core_rule_id,
            conversion_status="AUTO_CONVERTIBLE",
            conversion_reasons=reasons,
            conversion_confidence=max(confidence, 0.70),
        )

    reasons.append("UNSUPPORTED_RULE_TYPE")
    return rule.with_updates(
        core_rule_id=core_rule_id,
        conversion_status="MANUAL_REQUIRED",
        conversion_reasons=reasons,
        conversion_confidence=confidence,
    )


def classify_rules(
    p21_rules: list[CanonicalRule],
    mappings: list[RuleMapping],
) -> list[CanonicalRule]:
    mapping_by_key = _mapping_by_rule_key(mappings)
    classified: list[CanonicalRule] = []
    for rule in p21_rules:
        classified.append(_classify(rule, mapping_by_key.get(p21_mapping_key(rule))))
    return classified
