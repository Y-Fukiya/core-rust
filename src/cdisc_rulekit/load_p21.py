from __future__ import annotations

import csv
import re
from pathlib import Path
from typing import Any

from .io_utils import normalize_blank, split_semicolon_list
from .models import CanonicalRule

CG_RE = re.compile(r"\bCG\d{4,}\b")

DOMAIN_JOIN_KEYS = (
    "config_version",
    "agency",
    "config_name",
    "standard_name",
    "standard_version",
    "rule_id",
    "source_xml_path",
)

RAW_CONDITION_FIELDS = (
    "target",
    "variable",
    "when",
    "if",
    "test",
    "where",
    "search",
    "from",
    "terms",
    "group_by",
    "matching",
    "optional",
    "ignore_context",
    "match_exact",
    "child_conditions_json",
    "all_attributes_json",
)


def extract_cg_ids(*values: object) -> list[str]:
    ids: set[str] = set()
    for value in values:
        text = normalize_blank(value)
        if text is None:
            continue
        ids.update(match.group(0).upper() for match in CG_RE.finditer(text))
    return sorted(ids)


def _normalized_rows(path: Path) -> tuple[list[dict[str, Any]], list[str]]:
    warnings: list[str] = []
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        for index, row in enumerate(reader, start=2):
            if None in row:
                warnings.append(f"{path}:{index}: extra columns were ignored")
            rows.append({str(key): normalize_blank(value) for key, value in row.items() if key is not None})
    return rows, warnings


def _domain_key(row: dict[str, Any]) -> tuple[str | None, ...]:
    return tuple(row.get(key) for key in DOMAIN_JOIN_KEYS)


def _active(value: object) -> bool:
    text = normalize_blank(value)
    if text is None:
        return False
    return text.lower() in {"1", "true", "yes", "y", "active"}


def _domain_map(path: Path | None) -> tuple[dict[tuple[str | None, ...], dict[str, set[str]]], list[str]]:
    if path is None:
        return {}, []
    rows, warnings = _normalized_rows(path)
    mapping: dict[tuple[str | None, ...], dict[str, set[str]]] = {}
    for row in rows:
        if not _active(row.get("active")):
            continue
        bucket = mapping.setdefault(_domain_key(row), {"domains": set(), "classes": set()})
        domain = normalize_blank(row.get("domain"))
        domain_class = normalize_blank(row.get("domain_class"))
        if domain:
            bucket["domains"].add(domain.upper())
        if domain_class:
            bucket["classes"].add(domain_class.upper())
    return mapping, warnings


def _values_for_cg_extraction(row: dict[str, Any]) -> list[Any]:
    fields = [
        "cdisc_cg_ids",
        "publisher_ids_normalized",
        "publisher_id_raw",
        "message",
        "description",
        "target",
        "variable",
        "when",
        "if",
        "test",
        "where",
        "search",
        "from",
        "terms",
    ]
    return [row.get(field) for field in fields]


def _variable_list(row: dict[str, Any]) -> list[str]:
    variables = split_semicolon_list(row.get("variable"))
    target = normalize_blank(row.get("target"))
    if target:
        variables.append(target)
    return sorted({variable.upper() for variable in variables})


def _raw_condition(row: dict[str, Any]) -> dict[str, Any]:
    return {field: row.get(field) for field in RAW_CONDITION_FIELDS if field in row}


def _canonical_rule(row: dict[str, Any], joined: dict[str, set[str]] | None) -> CanonicalRule:
    row_domains = {domain.upper() for domain in split_semicolon_list(row.get("domains"))}
    row_classes = {klass.upper() for klass in split_semicolon_list(row.get("domain_classes"))}
    if joined:
        row_domains.update(joined["domains"])
        row_classes.update(joined["classes"])

    rule_id = row.get("rule_id")
    source_rule_id = rule_id or ""
    return CanonicalRule(
        source="P21",
        source_rule_id=source_rule_id,
        p21_rule_id=rule_id,
        cdisc_rule_ids=extract_cg_ids(*_values_for_cg_extraction(row)),
        standard_name=row.get("standard_name"),
        standard_version=row.get("standard_version"),
        agency=row.get("agency"),
        category=row.get("category"),
        severity=row.get("severity_type"),
        p21_rule_type=row.get("rule_type"),
        message=row.get("message"),
        description=row.get("description"),
        domains=sorted(row_domains),
        classes=sorted(row_classes),
        variables=_variable_list(row),
        target=row.get("target"),
        raw_condition=_raw_condition(row),
        source_path=row.get("source_xml_path"),
        raw_record=row,
    )


def load_p21_rules(
    rules_path: str | Path,
    domain_map_path: str | Path | None = None,
) -> tuple[list[CanonicalRule], list[str]]:
    path = Path(rules_path)
    rows, warnings = _normalized_rows(path)
    domain_mapping, domain_warnings = _domain_map(Path(domain_map_path) if domain_map_path else None)
    warnings.extend(domain_warnings)

    rules = [_canonical_rule(row, domain_mapping.get(_domain_key(row))) for row in rows]
    return rules, warnings
