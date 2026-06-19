from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class P21Predicate:
    variable: str
    check: dict[str, Any]
    positive_value: str
    negative_value: str


@dataclass(frozen=True)
class P21Guard:
    variable: str
    check: dict[str, Any]
    value: str


@dataclass(frozen=True)
class P21GuardGroup:
    checks: list[dict[str, Any]]
    values: dict[str, str]


_SIMPLE_COMPARISON = re.compile(
    r"^\s*(?P<variable>[A-Za-z][A-Za-z0-9_]*)\s*"
    r"(?P<operator>==|=|!=|<>|@eqic|@neqic|@ne|@re)\s*"
    r"(?P<value>.*?)\s*$",
    re.IGNORECASE,
)
_IDENTIFIER = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")


def _is_simple_same_record_expression(expression: object) -> bool:
    text = str(expression or "").strip()
    return bool(text) and ":" not in text and "@and" not in text.lower() and "@or" not in text.lower()


def _literal(value: str) -> tuple[str, bool]:
    text = value.strip()
    if (text.startswith("'") and text.endswith("'")) or (text.startswith('"') and text.endswith('"')):
        return text[1:-1], True
    return text, False


def _unsupported_column_comparator(value: str, quoted: bool) -> bool:
    return not quoted and bool(_IDENTIFIER.fullmatch(value))


def parse_expected_test(expression: object) -> P21Predicate | None:
    if not _is_simple_same_record_expression(expression):
        return None
    match = _SIMPLE_COMPARISON.fullmatch(str(expression or ""))
    if not match:
        return None

    variable = match.group("variable").upper()
    operator = match.group("operator").lower()
    value, quoted = _literal(match.group("value"))

    if operator in {"!=", "<>", "@ne"} and value == "":
        return P21Predicate(variable, {"name": variable, "operator": "is_empty"}, "Y", "")
    if operator in {"==", "="} and value == "":
        return P21Predicate(variable, {"name": variable, "operator": "is_not_empty"}, "", "Y")
    if operator != "@re" and _unsupported_column_comparator(value, quoted):
        return None
    if operator in {"==", "="}:
        return P21Predicate(variable, {"name": variable, "operator": "not_equal_to", "value": value}, value, "__INVALID__")
    if operator == "@eqic":
        return P21Predicate(
            variable,
            {"name": variable, "operator": "not_equal_to_case_insensitive", "value": value},
            value,
            "__INVALID__",
        )
    if operator in {"!=", "<>", "@ne"}:
        return P21Predicate(variable, {"name": variable, "operator": "equal_to", "value": value}, "__OTHER__", value)
    if operator == "@re" and value:
        positive = "1" if r"\d" in value or "[0-9]" in value else "VALID"
        return P21Predicate(
            variable,
            {"name": variable, "operator": "does_not_match_regex", "value": value},
            positive,
            "__INVALID__",
        )
    return None


def parse_when_guard(expression: object) -> P21Guard | None:
    if not _is_simple_same_record_expression(expression):
        return None
    match = _SIMPLE_COMPARISON.fullmatch(str(expression or ""))
    if not match:
        return None

    variable = match.group("variable").upper()
    operator = match.group("operator").lower()
    value, quoted = _literal(match.group("value"))

    if operator in {"==", "="} and value == "":
        return P21Guard(variable, {"name": variable, "operator": "is_empty"}, "")
    if operator in {"!=", "<>", "@ne"} and value == "":
        return P21Guard(variable, {"name": variable, "operator": "is_not_empty"}, "Y")
    if operator != "@re" and _unsupported_column_comparator(value, quoted):
        return None
    if operator in {"==", "="}:
        return P21Guard(variable, {"name": variable, "operator": "equal_to", "value": value}, value)
    if operator == "@eqic":
        return P21Guard(variable, {"name": variable, "operator": "equal_to_case_insensitive", "value": value}, value)
    if operator in {"!=", "<>", "@ne"}:
        return P21Guard(variable, {"name": variable, "operator": "not_equal_to", "value": value}, "__OTHER__")
    if operator == "@neqic":
        return P21Guard(variable, {"name": variable, "operator": "not_equal_to_case_insensitive", "value": value}, "__OTHER__")
    return None


def parse_when_guard_group(expression: object) -> P21GuardGroup | None:
    text = str(expression or "").strip()
    if not text:
        return P21GuardGroup([], {})

    has_and = bool(re.search(r"@and", text, flags=re.IGNORECASE))
    has_or = bool(re.search(r"@or", text, flags=re.IGNORECASE))
    if has_and and has_or:
        return None

    if has_and:
        guards = [parse_when_guard(part) for part in re.split(r"\s*@and\s*", text, flags=re.IGNORECASE)]
        if any(guard is None for guard in guards):
            return None
        parsed = [guard for guard in guards if guard is not None]
        return P21GuardGroup([guard.check for guard in parsed], {guard.variable: guard.value for guard in parsed})

    if has_or:
        guards = [parse_when_guard(part) for part in re.split(r"\s*@or\s*", text, flags=re.IGNORECASE)]
        if any(guard is None for guard in guards):
            return None
        parsed = [guard for guard in guards if guard is not None]
        if not parsed:
            return None
        return P21GuardGroup(
            [{"any": [guard.check for guard in parsed]}],
            {parsed[0].variable: parsed[0].value},
        )

    guard = parse_when_guard(text)
    if guard is None:
        return None
    return P21GuardGroup([guard.check], {guard.variable: guard.value})


def infer_condition_target(expression: object) -> str | None:
    predicate = parse_expected_test(expression)
    return predicate.variable if predicate else None
