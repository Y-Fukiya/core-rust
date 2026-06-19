from __future__ import annotations

import re
from difflib import SequenceMatcher

from .models import CanonicalRule, RuleMapping


def standard_key(value: str | None) -> str:
    if not value:
        return ""
    return re.sub(r"[^A-Z0-9]", "", value.upper())


def token_similarity(left: str | None, right: str | None) -> float:
    if not left or not right:
        return 0.0
    left_text = left.upper()
    right_text = right.upper()
    sequence = SequenceMatcher(None, left_text, right_text).ratio()
    left_tokens = set(re.findall(r"[A-Z0-9]+", left_text))
    right_tokens = set(re.findall(r"[A-Z0-9]+", right_text))
    if not left_tokens or not right_tokens:
        token_score = 0.0
    else:
        token_score = len(left_tokens & right_tokens) / len(left_tokens | right_tokens)
    return max(sequence, token_score)


def overlap(left: list[str], right: list[str]) -> list[str]:
    return sorted({item.upper() for item in left} & {item.upper() for item in right})


def _cg_mapping(p21_rule: CanonicalRule, core_rules: list[CanonicalRule]) -> RuleMapping | None:
    best: RuleMapping | None = None
    for core_rule in core_rules:
        cdisc_overlap = overlap(p21_rule.cdisc_rule_ids, core_rule.cdisc_rule_ids)
        if not cdisc_overlap:
            continue
        standard_match = standard_key(p21_rule.standard_name) == standard_key(core_rule.standard_name)
        domain_overlap = overlap(p21_rule.domains, core_rule.domains)
        variable_overlap = overlap(p21_rule.variables, core_rule.variables)
        confidence = 0.90
        if standard_match:
            confidence += 0.03
        if domain_overlap:
            confidence += 0.03
        if variable_overlap:
            confidence += 0.04
        candidate = RuleMapping(
            p21_rule_id=p21_rule.p21_rule_id or p21_rule.source_rule_id,
            core_rule_id=core_rule.core_rule_id,
            match_type="CG_ID",
            confidence=min(confidence, 1.0),
            cdisc_rule_id_overlap=cdisc_overlap,
            standard_match=standard_match,
            domain_overlap=domain_overlap,
            variable_overlap=variable_overlap,
            message_similarity=token_similarity(p21_rule.message, core_rule.message),
            notes=["Matched by shared CDISC rule identifier"],
        )
        if best is None or candidate.confidence > best.confidence:
            best = candidate
    return best


def _fuzzy_score(p21_rule: CanonicalRule, core_rule: CanonicalRule) -> tuple[float, bool, list[str], list[str], float]:
    standard_match = standard_key(p21_rule.standard_name) == standard_key(core_rule.standard_name)
    domain_overlap = overlap(p21_rule.domains, core_rule.domains)
    variable_overlap = overlap(p21_rule.variables, core_rule.variables)
    message_similarity = token_similarity(p21_rule.message, core_rule.message)
    description_similarity = token_similarity(p21_rule.description, core_rule.description)

    score = 0.0
    if standard_match:
        score += 0.20
    if domain_overlap:
        score += 0.20
    if variable_overlap:
        score += 0.25
    score += message_similarity * 0.25
    score += description_similarity * 0.10
    return min(score, 1.0), standard_match, domain_overlap, variable_overlap, message_similarity


def _fuzzy_mapping(p21_rule: CanonicalRule, core_rules: list[CanonicalRule]) -> RuleMapping | None:
    best: RuleMapping | None = None
    for core_rule in core_rules:
        score, standard_match, domain_overlap, variable_overlap, message_similarity = _fuzzy_score(
            p21_rule,
            core_rule,
        )
        if score < 0.60:
            continue
        candidate = RuleMapping(
            p21_rule_id=p21_rule.p21_rule_id or p21_rule.source_rule_id,
            core_rule_id=core_rule.core_rule_id,
            match_type="FUZZY",
            confidence=round(score, 4),
            standard_match=standard_match,
            domain_overlap=domain_overlap,
            variable_overlap=variable_overlap,
            message_similarity=round(message_similarity, 4),
            notes=["Fuzzy candidate only; not native coverage evidence"],
        )
        if best is None or candidate.confidence > best.confidence:
            best = candidate
    return best


def map_p21_to_core(
    p21_rules: list[CanonicalRule],
    core_rules: list[CanonicalRule],
) -> list[RuleMapping]:
    mappings: list[RuleMapping] = []
    for p21_rule in p21_rules:
        p21_rule_id = p21_rule.p21_rule_id or p21_rule.source_rule_id
        mapping = _cg_mapping(p21_rule, core_rules) or _fuzzy_mapping(p21_rule, core_rules)
        if mapping is None:
            mapping = RuleMapping(
                p21_rule_id=p21_rule_id,
                core_rule_id=None,
                match_type="NONE",
                confidence=0.0,
                notes=["No adequate CORE candidate found"],
            )
        mappings.append(mapping)
    return mappings
