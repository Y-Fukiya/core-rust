from __future__ import annotations

from typing import Any

from .io_utils import normalize_blank
from .models import CanonicalRule, OperatorInventoryItem

METADATA_KEYS = {
    "name",
    "value",
    "values",
    "variable",
    "variables",
    "dataset",
    "domain",
    "message",
    "description",
    "label",
    "type",
    "length",
}


def _collect_names(node: Any) -> list[str]:
    names: set[str] = set()
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "name":
                text = normalize_blank(value)
                if text:
                    names.add(text.upper())
            else:
                names.update(_collect_names(value))
    elif isinstance(node, list):
        for item in node:
            names.update(_collect_names(item))
    return sorted(names)


def _walk(
    node: Any,
    path: str,
    rule: CanonicalRule,
    items: list[OperatorInventoryItem],
) -> None:
    if isinstance(node, dict):
        keys = sorted(str(key) for key in node.keys())
        for key, value in node.items():
            key_text = str(key)
            child_path = f"{path}.{key_text}" if path else key_text
            if key_text.lower() not in METADATA_KEYS:
                items.append(
                    OperatorInventoryItem(
                        core_rule_id=rule.core_rule_id or rule.source_rule_id,
                        source_path=rule.source_path or "",
                        operator=key_text,
                        path=child_path,
                        node_kind="dict",
                        name_values=_collect_names(value),
                        raw_keys=keys,
                    )
                )
            _walk(value, child_path, rule, items)
    elif isinstance(node, list):
        for index, item in enumerate(node):
            _walk(item, f"{path}[{index}]", rule, items)


def build_operator_inventory(core_rules: list[CanonicalRule]) -> list[OperatorInventoryItem]:
    items: list[OperatorInventoryItem] = []
    for rule in core_rules:
        check = rule.raw_condition.get("Check")
        _walk(check, "Check", rule, items)
    return items
