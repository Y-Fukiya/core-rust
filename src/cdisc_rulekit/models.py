from __future__ import annotations

from dataclasses import asdict, dataclass, field, replace
from typing import Any


def _as_list(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, list):
        return [str(item) for item in value if item is not None]
    if isinstance(value, str):
        return [value] if value else []
    return [str(value)]


def _as_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


@dataclass(frozen=True)
class CanonicalRule:
    source: str
    source_rule_id: str
    source_rule_key: str | None = None
    p21_rule_id: str | None = None
    core_rule_id: str | None = None
    cdisc_rule_ids: list[str] = field(default_factory=list)
    standard_name: str | None = None
    standard_version: str | None = None
    substandard: str | None = None
    agency: str | None = None
    category: str | None = None
    severity: str | None = None
    p21_rule_type: str | None = None
    core_rule_type: str | None = None
    message: str | None = None
    description: str | None = None
    domains: list[str] = field(default_factory=list)
    classes: list[str] = field(default_factory=list)
    variables: list[str] = field(default_factory=list)
    target: str | None = None
    raw_condition: dict[str, Any] = field(default_factory=dict)
    parsed_condition: dict[str, Any] | None = None
    source_path: str | None = None
    raw_record: dict[str, Any] = field(default_factory=dict)
    conversion_status: str | None = None
    conversion_reasons: list[str] = field(default_factory=list)
    conversion_confidence: float | None = None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, row: dict[str, Any]) -> "CanonicalRule":
        values = dict(row)
        for key in ("cdisc_rule_ids", "domains", "classes", "variables", "conversion_reasons"):
            values[key] = _as_list(values.get(key))
        for key in ("raw_condition", "raw_record"):
            values[key] = _as_dict(values.get(key))
        parsed_condition = values.get("parsed_condition")
        values["parsed_condition"] = parsed_condition if isinstance(parsed_condition, dict) else None
        return cls(**values)

    def with_updates(self, **updates: Any) -> "CanonicalRule":
        return replace(self, **updates)


@dataclass(frozen=True)
class RuleMapping:
    p21_rule_id: str
    core_rule_id: str | None
    match_type: str
    confidence: float
    p21_rule_key: str | None = None
    cdisc_rule_id_overlap: list[str] = field(default_factory=list)
    standard_match: bool = False
    domain_overlap: list[str] = field(default_factory=list)
    variable_overlap: list[str] = field(default_factory=list)
    message_similarity: float | None = None
    notes: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, row: dict[str, Any]) -> "RuleMapping":
        values = dict(row)
        for key in ("cdisc_rule_id_overlap", "domain_overlap", "variable_overlap", "notes"):
            values[key] = _as_list(values.get(key))
        confidence = values.get("confidence", 0)
        values["confidence"] = float(confidence) if confidence is not None else 0.0
        return cls(**values)


@dataclass(frozen=True)
class OperatorInventoryItem:
    core_rule_id: str
    source_path: str
    operator: str
    path: str
    node_kind: str
    name_values: list[str] = field(default_factory=list)
    raw_keys: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
