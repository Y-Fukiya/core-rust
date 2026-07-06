"""Best-effort local Pinnacle 21 configuration XML extraction.

Installed environments should use defusedxml. The source-tree smoke fallback is
intentional before dependencies are installed; DTD/entity preflight runs before
parsing in either backend.
"""

from __future__ import annotations

import re
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

try:
    from defusedxml.common import DefusedXmlException
    from defusedxml import ElementTree as DefusedET
except ImportError:  # pragma: no cover - optional hardening dependency.
    # pyproject.toml declares defusedxml for installed environments. The
    # fallback keeps source-tree smoke tests usable before dependencies are
    # installed; DTD/entity declarations are still rejected before parsing.
    DefusedXmlException = None
    DefusedET = None

XML_PARSE_EXCEPTIONS = (
    (ET.ParseError, DefusedXmlException) if DefusedXmlException is not None else (ET.ParseError,)
)
from .errors import CliUsageError
from .io_utils import normalize_blank, split_semicolon_list
from .load_p21 import extract_cg_ids
from .models import CanonicalRule


# Top-level configuration files may use <check> as the rule element name.
# Nested <check> children under a rule are kept as raw child values instead.
RULE_TAGS = {"rule", "validationrule", "validation_rule", "check"}
MAX_CONFIG_BYTES = 20 * 1024 * 1024
SAFE_SOURCE_LABEL_RE = re.compile(r"^[A-Za-z0-9_.:-]+$")


def load_p21_config_rules(
    paths: list[str | Path],
    source_labels: list[str] | None = None,
) -> tuple[list[CanonicalRule], list[str]]:
    if source_labels is not None and len(source_labels) != len(paths):
        raise CliUsageError("--source-label count must match --input count")
    rules: list[CanonicalRule] = []
    warnings: list[str] = []
    for index, path in enumerate(paths, start=1):
        if source_labels is None:
            source_label = _default_source_label(index, Path(path))
        else:
            source_label = _safe_source_label(source_labels[index - 1])
        loaded, loaded_warnings = _load_p21_config_path(Path(path), source_label)
        rules.extend(loaded)
        warnings.extend(loaded_warnings)
    return rules, warnings


def write_p21_config_extraction_report(
    path: str | Path,
    rules: list[CanonicalRule],
    warnings: list[str],
    input_count: int | None = None,
) -> None:
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    source_labels_with_rules = {rule.source_path for rule in rules if rule.source_path}
    lines = [
        "# P21 Config Extraction Report",
        "",
        "This report describes a local, user-supplied conversion artifact.",
        "The converter does not download, fetch, scrape, or bundle Pinnacle 21 configuration files.",
        "Only use source XML files and generated catalogs that you are licensed and permitted to process.",
        "Extraction is best-effort; review the generated CSV/JSONL before using it as a P21PORT catalog.",
        "",
        f"- Input files processed: `{input_count if input_count is not None else len(source_labels_with_rules)}`",
        f"- Source labels with extracted rules: `{len(source_labels_with_rules)}`",
        f"- Extracted rules: `{len(rules)}`",
        f"- Warnings: `{len(warnings)}`",
    ]
    if warnings:
        lines.extend(["", "## Warnings", ""])
        lines.extend(f"- {warning}" for warning in warnings)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _load_p21_config_path(path: Path, source_label: str) -> tuple[list[CanonicalRule], list[str]]:
    try:
        root = _parse_config_xml(path)
    except XML_PARSE_EXCEPTIONS as exc:
        raise CliUsageError(f"{path}: malformed XML configuration: {exc}") from exc
    rules, warnings = _collect_rules(source_label, root, _attributes(root), {})
    if not rules and not warnings:
        warnings.append(f"{source_label}: no rule elements found")
    return rules, warnings


def _collect_rules(
    source_label: str,
    element: ET.Element,
    config: dict[str, str],
    standard: dict[str, str],
) -> tuple[list[CanonicalRule], list[str]]:
    local_name = _local_name(element.tag)
    next_standard = _attributes(element) if local_name in {"standard", "standardconfig"} else standard
    if local_name in RULE_TAGS:
        return _canonical_rule(source_label, config, standard, element)

    rules: list[CanonicalRule] = []
    warnings: list[str] = []
    for child in list(element):
        child_rules, child_warnings = _collect_rules(source_label, child, config, next_standard)
        rules.extend(child_rules)
        warnings.extend(child_warnings)
    return rules, warnings


def _canonical_rule(
    source_label: str,
    config: dict[str, str],
    standard: dict[str, str],
    element: ET.Element,
) -> tuple[list[CanonicalRule], list[str]]:
    attrs = _attributes(element)
    child_values = _direct_child_values(element)

    rule_id = _first(attrs, child_values, "id", "rule_id", "ruleid", "rule") or ""
    if not rule_id:
        return [], [f"{source_label}: rule element without id was skipped"]

    standard_name = (
        _first(standard, {}, "name", "standard_name", "standard")
        or _first(config, {}, "name", "config_name", "standard_name", "standard")
    )
    standard_version = _first(standard, {}, "version", "standard_version") or _first(
        config,
        {},
        "standard_version",
        "version",
    )
    agency = _first(attrs, child_values, "agency") or _first(config, {}, "agency")
    message = _first(attrs, child_values, "message", "error_message", "text")
    description = _first(attrs, child_values, "description", "desc", "explanation")
    target = _first(attrs, child_values, "target", "variable", "var")
    raw_condition = {
        key: value
        for key, value in {
            "target": target,
            "variable": _first(attrs, child_values, "variable", "variables", "var"),
            "when": _first(attrs, child_values, "when", "if", "where"),
            "test": _first(attrs, child_values, "test", "condition", "expression", "logic"),
        }.items()
        if value
    }

    raw_record: dict[str, Any] = {
        **{f"@{key}": value for key, value in attrs.items()},
        **child_values,
        "source_xml_file": source_label,
    }

    return [
        CanonicalRule(
            source="P21",
            source_rule_id=rule_id,
            source_rule_key="|".join(
                [
                    _first(config, {}, "version", "config_version") or "",
                    agency or "",
                    _first(config, {}, "name", "config_name") or "",
                    standard_name or "",
                    standard_version or "",
                    rule_id,
                    source_label,
                ],
            ),
            p21_rule_id=rule_id,
            cdisc_rule_ids=extract_cg_ids(rule_id, message, description, *raw_condition.values()),
            standard_name=standard_name,
            standard_version=standard_version,
            agency=agency,
            category=_first(attrs, child_values, "category"),
            severity=_first(attrs, child_values, "severity", "severity_type"),
            p21_rule_type=_first(attrs, child_values, "type", "rule_type"),
            message=message,
            description=description,
            domains=_split_values(_first(attrs, child_values, "domain", "domains", "domainlist", "domain_list")),
            classes=_split_values(
                _first(
                    attrs,
                    child_values,
                    "class",
                    "classes",
                    "classlist",
                    "class_list",
                    "domain_class",
                    "domain_classes",
                    "domainclasslist",
                    "domain_class_list",
                ),
            ),
            variables=sorted(
                {
                    value.upper()
                    for value in _split_values(
                        _first(attrs, child_values, "variable", "variables", "variablelist", "variable_list", "var"),
                    )
                },
            ),
            target=target.upper() if target else None,
            raw_condition=raw_condition,
            source_path=source_label,
            raw_record=raw_record,
        ),
    ], []


def _parse_config_xml(path: Path) -> ET.Element:
    if not path.exists():
        raise CliUsageError(f"{path}: XML configuration file does not exist")
    # Symlink-to-file inputs are accepted because this converter only processes
    # explicit local user-supplied XML paths; non-file inputs remain rejected.
    if not path.is_file():
        raise CliUsageError(f"{path}: XML configuration input must be a regular file")
    if path.stat().st_size > MAX_CONFIG_BYTES:
        raise CliUsageError(f"{path}: XML configuration exceeds {MAX_CONFIG_BYTES} bytes")
    try:
        payload = path.read_bytes()
    except OSError as exc:
        raise CliUsageError(f"{path}: could not read XML configuration: {exc}") from exc
    if len(payload) > MAX_CONFIG_BYTES:
        raise CliUsageError(f"{path}: XML configuration exceeds {MAX_CONFIG_BYTES} bytes")
    lowered = payload.lower()
    if b"<!doctype" in lowered or b"<!entity" in lowered:
        raise CliUsageError(f"{path}: DTD/entity declarations are not supported")
    parser = DefusedET if DefusedET is not None else ET
    return parser.fromstring(payload)


def _attributes(element: ET.Element) -> dict[str, str]:
    values: dict[str, str] = {}
    for key, value in element.attrib.items():
        normalized = normalize_blank(value)
        if normalized:
            values[_normalize_key(key)] = normalized
    return values


def _direct_child_values(element: ET.Element) -> dict[str, str]:
    values: dict[str, str] = {}
    for child in list(element):
        key = _local_name(child.tag)
        if list(child):
            # Keep nested helper elements (including <check> under <rule>) as
            # raw child values instead of treating them as separate rules.
            nested = _nested_collection_value(child)
            if nested:
                values[key] = nested
            continue
        text = normalize_blank(child.text)
        if text:
            values[key] = _join_value(values.get(key), text)
    return values


def _nested_collection_value(element: ET.Element) -> str | None:
    key = _local_name(element.tag)
    expected_children = {
        "domains": {"domain"},
        "domainlist": {"domain", "item", "value"},
        "domain_list": {"domain", "item", "value"},
        "classes": {"class", "domain_class"},
        "classlist": {"class", "domain_class", "item", "value"},
        "class_list": {"class", "domain_class", "item", "value"},
        "domain_classes": {"class", "domain_class"},
        "domainclasslist": {"class", "domain_class", "item", "value"},
        "domain_class_list": {"class", "domain_class", "item", "value"},
        "variables": {"variable", "var"},
        "variablelist": {"variable", "var", "item", "value"},
        "variable_list": {"variable", "var", "item", "value"},
    }.get(key)
    if not expected_children:
        return None

    values = []
    for child in list(element):
        if list(child) or _local_name(child.tag) not in expected_children:
            continue
        text = normalize_blank(child.text)
        if text:
            values.append(text)
    return ";".join(values) if values else None


def _join_value(existing: str | None, value: str) -> str:
    if not existing:
        return value
    return f"{existing};{value}"


def _first(attrs: dict[str, str], children: dict[str, str], *keys: str) -> str | None:
    for key in keys:
        normalized = _normalize_key(key)
        value = attrs.get(normalized) or children.get(normalized)
        if value:
            return value
    return None


def _split_values(value: object) -> list[str]:
    values = split_semicolon_list(value)
    if len(values) == 1 and "," in values[0]:
        values = [part.strip() for part in values[0].split(",") if normalize_blank(part)]
    return sorted({value.upper() for value in values})


def _local_name(value: str) -> str:
    return _normalize_key(value.rsplit("}", 1)[-1])


def _normalize_key(value: str) -> str:
    return "".join(ch.lower() if ch.isalnum() else "_" for ch in value).strip("_")


def _safe_source_label(value: str) -> str:
    label = normalize_blank(value)
    if not label:
        raise CliUsageError("--source-label values must not be blank")
    if not SAFE_SOURCE_LABEL_RE.fullmatch(label):
        raise CliUsageError("--source-label values may contain only letters, numbers, dot, underscore, colon, or hyphen")
    return label


def _default_source_label(index: int, path: Path) -> str:
    filename = re.sub(r"[^A-Za-z0-9_.:-]+", "_", path.name).strip("_.:-")
    if not filename:
        filename = "input_xml"
    return f"source_{index:03d}:{filename}"
