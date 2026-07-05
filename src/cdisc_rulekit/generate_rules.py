from __future__ import annotations

import csv
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

from .errors import CliUsageError
from .io_utils import ensure_dir, split_semicolon_list, write_csv
from .map_rules import standard_key
from .models import CanonicalRule
from .p21_condition import (
    infer_condition_target,
    infer_condition_variables,
    parse_expected_check_group,
    parse_expected_test,
    parse_when_guard_group,
)

GENERATABLE_TYPES = {"MATCH", "REGEX", "REQUIRED", "FIND", "CONDITION", "UNIQUE"}
BASE_COLUMNS = ["STUDYID", "DOMAIN", "USUBJID"]
CORE_OPERATOR_ALIASES = {
    "does_not_match_regex": "not_matches_regex",
    "is_empty": "empty",
    "is_not_empty": "non_empty",
}
UNSUPPORTED_RUST_REGEX_PATTERNS = [
    re.compile(r"(?<!\\)\(\?([=!]|<[=!])"),
    re.compile(r"(?<!\\)\\[1-9]"),
    re.compile(r"(?<!\\)\\k<"),
]
SUMMARY_FIELDS = [
    "source_rule_key",
    "p21_rule_id",
    "generated_rule_id",
    "generation_status",
    "skip_reason",
    "p21_rule_type",
    "domain",
    "variable",
]


@dataclass(frozen=True)
class GenerationSummary:
    rows: list[dict[str, object]]

    @property
    def generated_count(self) -> int:
        return sum(1 for row in self.rows if row["generation_status"] == "GENERATED")

    @property
    def skipped_count(self) -> int:
        return sum(1 for row in self.rows if row["generation_status"] == "SKIPPED")


def generated_rule_id(rule: CanonicalRule) -> str:
    standard = standard_key(rule.standard_name) or "UNKNOWN"
    source = rule.source_rule_key or rule.source_rule_id
    digest = hashlib.sha1(source.encode("utf-8")).hexdigest()[:8].upper()
    p21_id = re.sub(r"[^A-Z0-9]+", "", (rule.p21_rule_id or rule.source_rule_id).upper())
    return f"P21PORT-{standard}-{p21_id}-{digest}"


def _product_name(rule: CanonicalRule) -> str:
    return standard_key(rule.standard_name) or "UNKNOWN"


def _version_value(rule: CanonicalRule) -> str:
    return (rule.standard_version or "0").replace(".", "-")


def _domain(rule: CanonicalRule) -> str | None:
    for domain in rule.domains:
        upper = domain.upper()
        if upper != "GLOBAL" and re.fullmatch(r"[A-Z][A-Z0-9]{1,7}", upper):
            return upper
    return None


def _variable(rule: CanonicalRule) -> str | None:
    candidates = [rule.target or "", *rule.variables]
    candidates.extend(infer_condition_variables(rule.raw_condition.get("test")))
    for variable in candidates:
        upper = variable.upper()
        if upper in {"DOMAIN", "DATASET", "METADATA", "VARIABLE"}:
            continue
        if re.fullmatch(r"[A-Z][A-Z0-9_]{1,31}", upper):
            return upper
    return None


def _required_operators(rule_type: str) -> set[str]:
    common = {"all", "equal_to"}
    if rule_type == "REQUIRED":
        return common | {"is_empty"}
    if rule_type == "REGEX":
        return common | {"does_not_match_regex"}
    if rule_type == "MATCH":
        return common | {"is_not_contained_by"}
    if rule_type == "FIND":
        return common | {"not_exists"}
    if rule_type == "CONDITION":
        return common
    if rule_type == "UNIQUE":
        return common | {"is_not_unique_set"}
    return common


def _operators_in_check(check: dict[str, Any]) -> set[str]:
    operators: set[str] = set()
    if "operator" in check:
        operators.add(str(check["operator"]))
    for key in ("all", "any"):
        for child in check.get(key, []) or []:
            if isinstance(child, dict):
                operators.update(_operators_in_check(child))
    return operators


def _unsupported_regex_in_check(check: dict[str, Any]) -> str | None:
    if check.get("operator") == "does_not_match_regex":
        reason = _unsupported_rust_regex_reason(check.get("value"))
        if reason:
            return reason
    for key in ("all", "any"):
        for child in check.get(key, []) or []:
            if isinstance(child, dict):
                reason = _unsupported_regex_in_check(child)
                if reason:
                    return reason
    return None


def _unsupported_operator(required: set[str], allowed: set[str]) -> str | None:
    for operator in sorted(required):
        if operator not in allowed:
            return operator
    return None


def _full_match_regex(pattern: object) -> object:
    if not isinstance(pattern, str):
        return pattern
    if pattern.startswith("^") and pattern.endswith("$"):
        return pattern
    return f"^(?:{pattern})$"


def _unsupported_rust_regex_reason(pattern: object) -> str | None:
    if not isinstance(pattern, str):
        return None
    if any(unsupported.search(pattern) for unsupported in UNSUPPORTED_RUST_REGEX_PATTERNS):
        return "UNSUPPORTED_RUST_REGEX_SYNTAX"
    return None


def _numeric_literal(value: object) -> object:
    if not isinstance(value, str):
        return value
    if not re.fullmatch(r"-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?", value):
        return value
    return float(value) if "." in value else int(value)


def _render_core_value(operator: str, key: str, value: object) -> object:
    if key != "value":
        return value
    if operator == "does_not_match_regex":
        return _full_match_regex(value)
    if operator in {
        "equal_to",
        "equal_to_case_insensitive",
        "not_equal_to",
        "not_equal_to_case_insensitive",
    }:
        return _numeric_literal(value)
    return value


def _render_core_check(check: dict[str, Any]) -> dict[str, Any]:
    operator = check.get("operator")
    if operator:
        operator_name = str(operator)
        rendered = {
            key: _render_core_value(operator_name, key, value)
            for key, value in check.items()
        }
        rendered["operator"] = CORE_OPERATOR_ALIASES.get(operator_name, operator_name)
        return rendered

    rendered: dict[str, Any] = {}
    for key in ("all", "any"):
        children = check.get(key)
        if children is not None:
            rendered[key] = [_render_core_check(child) for child in children]
    return rendered


def _regex_pattern(rule: CanonicalRule) -> str | None:
    value = rule.raw_condition.get("test")
    return str(value) if value not in (None, "") else None


def _match_terms(rule: CanonicalRule) -> list[str]:
    return split_semicolon_list(rule.raw_condition.get("terms"))


def _parse_literal_equality(expression: object) -> tuple[str, str] | None:
    text = str(expression or "").strip()
    match = re.fullmatch(r"([A-Za-z][A-Za-z0-9_]*)\s*(?:=|==)\s*['\"]?([^'\"]+)['\"]?", text)
    if not match:
        return None
    return match.group(1).upper(), match.group(2)


def _parse_non_empty_check(expression: object) -> str | None:
    text = str(expression or "").strip()
    patterns = [
        r"([A-Za-z][A-Za-z0-9_]*)\s*(?:!=|<>)\s*['\"]{2}",
        r"([A-Za-z][A-Za-z0-9_]*)\s+is\s+not\s+(?:null|empty)",
    ]
    for pattern in patterns:
        match = re.fullmatch(pattern, text, flags=re.IGNORECASE)
        if match:
            return match.group(1).upper()
    return None


def _simple_condition_parts(rule: CanonicalRule, variable: str) -> tuple[tuple[str, str], str] | None:
    equality = _parse_literal_equality(rule.raw_condition.get("when") or rule.raw_condition.get("if"))
    target = _parse_non_empty_check(rule.raw_condition.get("test")) or variable
    if not equality or target.upper() != variable:
        return None
    return equality, target


def _condition_checks(rule: CanonicalRule, variable: str) -> tuple[list[dict[str, Any]] | None, str | None]:
    group = parse_expected_check_group(rule.raw_condition.get("test"))
    if group and variable in group.variables:
        checks: list[dict[str, Any]] = []
        when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
        if when not in (None, ""):
            guard_group = parse_when_guard_group(when)
            if not guard_group:
                return None, "MISSING_SIMPLE_SAME_RECORD_CONDITION"
            checks.extend(guard_group.checks)
        checks.extend(group.checks)
        return checks, None

    predicate = parse_expected_test(rule.raw_condition.get("test"))
    if predicate and predicate.variable == variable:
        checks: list[dict[str, Any]] = []
        when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
        if when not in (None, ""):
            guard_group = parse_when_guard_group(when)
            if not guard_group:
                return None, "MISSING_SIMPLE_SAME_RECORD_CONDITION"
            checks.extend(guard_group.checks)
        checks.append(predicate.check)
        return checks, None

    condition_parts = _simple_condition_parts(rule, variable)
    if not condition_parts:
        return None, "MISSING_SIMPLE_SAME_RECORD_CONDITION"
    (condition_variable, condition_value), target = condition_parts
    return [
        {"name": condition_variable, "operator": "equal_to", "value": condition_value},
        {"name": target, "operator": "is_empty"},
    ], None


def _unique_group_by(rule: CanonicalRule) -> list[str]:
    raw = rule.raw_condition.get("group_by")
    values = split_semicolon_list(str(raw or "").replace(",", ";"))
    group_by = [
        value.upper()
        for value in values
        if re.fullmatch(r"[A-Z][A-Z0-9_]{1,31}", value.upper())
    ]
    return group_by or ["DOMAIN"]


def _build_check(rule: CanonicalRule, domain: str, variable: str) -> tuple[dict[str, Any] | None, str | None]:
    rule_type = (rule.p21_rule_type or "").upper()
    checks: list[dict[str, Any]] = []
    if rule_type == "REQUIRED":
        checks.append({"name": variable, "operator": "is_empty"})
    elif rule_type == "REGEX":
        pattern = _regex_pattern(rule)
        if not pattern:
            return None, "MISSING_REGEX_PATTERN"
        unsupported_regex = _unsupported_rust_regex_reason(pattern)
        if unsupported_regex:
            return None, unsupported_regex
        checks.append({"name": variable, "operator": "does_not_match_regex", "value": pattern})
    elif rule_type == "MATCH":
        terms = _match_terms(rule)
        if not terms:
            return None, "MISSING_MATCH_TERMS"
        checks.append({"name": variable, "operator": "is_not_contained_by", "value": terms})
    elif rule_type == "FIND":
        checks.append({"name": variable, "operator": "not_exists"})
    elif rule_type == "CONDITION":
        condition_checks, error = _condition_checks(rule, variable)
        if error or condition_checks is None:
            return None, error or "MISSING_SIMPLE_SAME_RECORD_CONDITION"
        checks.extend(condition_checks)
    elif rule_type == "UNIQUE":
        group_by = _unique_group_by(rule)
        if not group_by:
            return None, "MISSING_UNIQUE_GROUP_BY"
        when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
        if when not in (None, ""):
            guard_group = parse_when_guard_group(when)
            if not guard_group:
                return None, "MISSING_SIMPLE_UNIQUE_GUARD"
            checks.extend(guard_group.checks)
        checks.append(
            {
                "name": variable,
                "operator": "is_not_unique_set",
                "value": group_by[0] if len(group_by) == 1 else group_by,
            },
        )
    else:
        return None, "UNSUPPORTED_RULE_TYPE"
    checks.append({"name": "DOMAIN", "operator": "equal_to", "value": domain})
    return {"all": checks}, None


def _rule_yml(rule: CanonicalRule, rule_id: str, domain: str, check: dict[str, Any]) -> dict[str, Any]:
    standard = _product_name(rule)
    standard_entry: dict[str, Any] = {"Name": standard}
    if rule.standard_version:
        standard_entry["Version"] = rule.standard_version
    if rule.cdisc_rule_ids:
        standard_entry["References"] = [
            {
                "Origin": "P21 Community",
                "Rule Identifier": {"Id": cdisc_id},
            }
            for cdisc_id in rule.cdisc_rule_ids
        ]

    payload: dict[str, Any] = {
        "Core": {
            "Id": rule_id,
            "Status": "Draft",
            "Version": "0.1",
            "Description": rule.description or rule.message or f"Draft port of {rule.p21_rule_id}",
        },
        "Authorities": [{"Organization": "CDISC", "Standards": [standard_entry]}],
        "Check": _render_core_check(check),
        "Outcome": {"Message": rule.message or f"{rule.p21_rule_id} violation"},
        "Rule Type": "Record Data",
        "Scope": {"Domains": {"Include": [domain]}},
        "Sensitivity": "Record",
    }
    if rule.classes:
        payload["Scope"]["Classes"] = {"Include": rule.classes}
    return payload


def _dataset_name(domain: str) -> str:
    return domain.lower()


def _positive_negative_values(rule: CanonicalRule, variable: str) -> tuple[str, str]:
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type == "MATCH":
        terms = _match_terms(rule)
        return (terms[0] if terms else "Y"), "__INVALID__"
    if rule_type == "REGEX":
        pattern = _regex_pattern(rule) or ""
        if "P(?:" in pattern or "P(?=" in pattern or "P\\d" in pattern or "P[0-9]" in pattern:
            return "P1D", "ABC"
        if r"\d" in pattern or "[0-9]" in pattern:
            return "1", "ABC"
        return "VALID", "invalid value"
    if rule_type == "FIND":
        return "Y", ""
    if _is_numeric_variable(variable):
        return "1", ""
    return "Y", ""


def _is_numeric_variable(variable: str) -> bool:
    upper = variable.upper()
    return upper.endswith("NUM") or upper.endswith("SEQ")


def _variable_type(variable: str) -> str:
    return "Num" if _is_numeric_variable(variable) else "Char"


def _condition_case_values(rule: CanonicalRule, variable: str, case_type: str) -> dict[str, str]:
    if (rule.p21_rule_type or "").upper() != "CONDITION":
        return {}
    group = parse_expected_check_group(rule.raw_condition.get("test"))
    if group and variable in group.variables:
        values = dict(group.positive_values if case_type == "positive" else group.negative_values)
        when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
        guard_group = parse_when_guard_group(when)
        if guard_group:
            values = _merge_guard_values(values, guard_group, case_type)
        return values

    predicate = parse_expected_test(rule.raw_condition.get("test"))
    if predicate and predicate.variable == variable:
        values = {
            variable: predicate.positive_value if case_type == "positive" else predicate.negative_value,
        }
        when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
        guard_group = parse_when_guard_group(when)
        if guard_group:
            values = _merge_guard_values(values, guard_group, case_type)
        return values

    condition_parts = _simple_condition_parts(rule, variable)
    if not condition_parts:
        return {}
    (condition_variable, condition_value), _target = condition_parts
    positive_value, negative_value = _positive_negative_values(rule, variable)
    return {
        condition_variable: condition_value,
        variable: positive_value if case_type == "positive" else negative_value,
    }


def _merge_guard_values(values: dict[str, str], guard_group: Any, case_type: str) -> dict[str, str]:
    if case_type != "negative":
        return {**guard_group.values, **values}

    merged = dict(values)
    for variable, guard_value in guard_group.values.items():
        current_value = merged.get(variable)
        guard_check = _find_check_for_variable(guard_group.checks, variable)
        if current_value is None or not _value_satisfies_check(current_value, guard_check):
            merged[variable] = guard_value
    return merged


def _find_check_for_variable(checks: list[dict[str, Any]], variable: str) -> dict[str, Any] | None:
    for check in checks:
        name = check.get("name")
        if isinstance(name, str) and name.upper() == variable.upper():
            return check
        for key in ("all", "any"):
            child_match = _find_check_for_variable(
                [child for child in check.get(key, []) or [] if isinstance(child, dict)],
                variable,
            )
            if child_match:
                return child_match
    return None


def _value_satisfies_check(value: str, check: dict[str, Any] | None) -> bool:
    if not check:
        return False
    operator = str(check.get("operator") or "")
    expected = str(check.get("value") or "")
    if operator in {"equal_to", "==", "="}:
        return value == expected
    if operator == "equal_to_case_insensitive":
        return value.casefold() == expected.casefold()
    if operator in {"not_equal_to", "!=", "<>"}:
        return value != expected
    if operator == "not_equal_to_case_insensitive":
        return value.casefold() != expected.casefold()
    if operator == "is_empty":
        return value == ""
    if operator == "is_not_empty":
        return value != ""
    return False


def _unique_case_rows(rule: CanonicalRule, domain: str, variable: str, case_type: str) -> list[dict[str, str]]:
    group_by = _unique_group_by(rule)
    guard_values: dict[str, str] = {}
    when = rule.raw_condition.get("when") or rule.raw_condition.get("if")
    guard_group = parse_when_guard_group(when)
    if guard_group:
        guard_values.update(guard_group.values)

    group_values = {}
    for group in group_by:
        if group == "STUDYID":
            group_values[group] = "CDISC-P21PORT"
        elif group == "DOMAIN":
            group_values[group] = domain
        else:
            group_values[group] = f"{group}-GROUP"
    positive_a, positive_b = (
        ("1", "2") if _is_numeric_variable(variable) else ("P21PORT-001", "P21PORT-002")
    )
    negative = "1" if _is_numeric_variable(variable) else "P21PORT-001"
    guarded_value = guard_values.get(variable)
    guard_check = _find_check_for_variable(guard_group.checks, variable) if guard_group else None
    guard_operator = str(guard_check.get("operator") or "") if guard_check else ""
    if guarded_value not in (None, "") and guard_operator in {"equal_to", "equal_to_case_insensitive"}:
        positive_a = guarded_value
        negative = guarded_value

    rows: list[dict[str, str]] = []
    for index in range(2):
        row = {
            "STUDYID": "CDISC-P21PORT",
            "DOMAIN": domain,
            "USUBJID": f"P21PORT-{index + 1:03d}",
        }
        row.update(group_values)
        row.update(guard_values)
        if case_type == "positive":
            row[variable] = positive_a if index == 0 else positive_b
        else:
            row[variable] = negative
        rows.append(row)
    return rows


def _write_data_case(rule_dir: Path, case_type: str, rule: CanonicalRule, domain: str, variable: str) -> None:
    data_dir = rule_dir / case_type / "01" / "data"
    ensure_dir(data_dir)
    dataset = _dataset_name(domain)
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type == "UNIQUE":
        rows = _unique_case_rows(rule, domain, variable, case_type)
        columns = list(dict.fromkeys(column for row in rows for column in row))
    else:
        positive_value, negative_value = _positive_negative_values(rule, variable)
        value = positive_value if case_type == "positive" else negative_value
        condition_values = _condition_case_values(rule, variable, case_type)
        include_target = not (rule_type == "FIND" and case_type == "negative")
        target_columns = [variable] if include_target else []
        columns = list(dict.fromkeys([*BASE_COLUMNS, *condition_values.keys(), *target_columns]))
        row = {column: "" for column in columns}
        row.update({"STUDYID": "CDISC-P21PORT", "DOMAIN": domain, "USUBJID": "P21PORT-001"})
        if include_target:
            row[variable] = value
        row.update(condition_values)
        rows = [row]

    (data_dir / ".env").write_text(
        f"PRODUCT={_product_name(rule)}\nVERSION={_version_value(rule)}\n",
        encoding="utf-8",
    )
    write_csv(data_dir / "_datasets.csv", [{"Filename": dataset, "Label": f"{domain} generated test data"}], ["Filename", "Label"])
    variable_rows = [
        {
            "dataset": dataset,
            "variable": column,
            "label": column,
            "type": _variable_type(column),
            "length": max(1, *(len(str(row.get(column, ""))) for row in rows)),
        }
        for column in columns
    ]
    write_csv(data_dir / "_variables.csv", variable_rows, ["dataset", "variable", "label", "type", "length"])
    write_csv(data_dir / f"{dataset}.csv", rows, columns)


def _expected_negative_variables(rule: CanonicalRule, variable: str) -> str:
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type == "FIND":
        return "DOMAIN"
    if rule_type == "CONDITION":
        check, _error = _build_check(rule, _domain(rule) or "", variable)
        if check:
            return "|".join(_check_variable_names(check))
    if rule_type == "UNIQUE":
        check, _error = _build_check(rule, _domain(rule) or "", variable)
        if check:
            return "|".join(_check_variable_names(check))
    return f"{variable}|DOMAIN"


def _check_variable_names(check: dict[str, Any]) -> list[str]:
    names: list[str] = []
    name = check.get("name")
    if isinstance(name, str) and name:
        names.append(name.upper())
    for key in ("all", "any"):
        for child in check.get(key, []) or []:
            if isinstance(child, dict):
                names.extend(_check_variable_names(child))
    return list(dict.fromkeys(names))


def _expected_negative_issue_count(rule: CanonicalRule) -> int:
    if (rule.p21_rule_type or "").upper() == "UNIQUE":
        return 2
    return 1


def _write_expected_results(rule_dir: Path, rule_id: str, rule: CanonicalRule, domain: str, variable: str) -> None:
    negative_variables = _expected_negative_variables(rule, variable)
    negative_count = _expected_negative_issue_count(rule)
    rows = [
        {
            "case_type": "positive",
            "case_id": "01",
            "issue_index": "",
            "expected_issue_count": 0,
            "rule_id": rule_id,
            "dataset": domain,
            "row": "",
            "variables": "",
        }
    ]
    rows.extend(
        {
            "case_type": "negative",
            "case_id": "01",
            "issue_index": index,
            "expected_issue_count": negative_count,
            "rule_id": rule_id,
            "dataset": domain,
            "row": index,
            "variables": negative_variables,
        }
        for index in range(1, negative_count + 1)
    )
    write_csv(
        rule_dir / "expected_results.csv",
        rows,
        ["case_type", "case_id", "issue_index", "expected_issue_count", "rule_id", "dataset", "row", "variables"],
    )


def _write_manifest(rule_dir: Path, rule: CanonicalRule, rule_id: str, warnings: list[str]) -> None:
    payload = {
        "generated_rule_id": rule_id,
        "source": "P21",
        "source_rule_id": rule.source_rule_id,
        "source_rule_key": rule.source_rule_key,
        "p21_rule_id": rule.p21_rule_id,
        "standard_name": rule.standard_name,
        "standard_version": rule.standard_version,
        "agency": rule.agency,
        "p21_rule_type": rule.p21_rule_type,
        "conversion_status": rule.conversion_status,
        "conversion_reasons": rule.conversion_reasons,
        "cdisc_rule_ids": rule.cdisc_rule_ids,
        "warnings": warnings,
    }
    (rule_dir / "manifest.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _skip_row(rule: CanonicalRule, reason: str) -> dict[str, object]:
    return {
        "source_rule_key": rule.source_rule_key,
        "p21_rule_id": rule.p21_rule_id,
        "generated_rule_id": "",
        "generation_status": "SKIPPED",
        "skip_reason": reason,
        "p21_rule_type": rule.p21_rule_type,
        "domain": "",
        "variable": "",
    }


def _generate_one(
    rule: CanonicalRule,
    root: Path,
    allowed_operators: set[str],
    *,
    include_fuzzy_candidates: bool = False,
) -> dict[str, object]:
    if rule.conversion_status != "AUTO_CONVERTIBLE":
        return _skip_row(rule, f"STATUS_NOT_GENERATABLE:{rule.conversion_status}")
    warnings = []
    if "FUZZY_CORE_CANDIDATE" in rule.conversion_reasons and not include_fuzzy_candidates:
        return _skip_row(rule, "FUZZY_CANDIDATE_REQUIRES_REVIEW")
    if "FUZZY_CORE_CANDIDATE" in rule.conversion_reasons:
        warnings.append("FUZZY_CORE_CANDIDATE_REQUIRES_REVIEW")
    if "UNSUPPORTED_RUST_REGEX_SYNTAX" in rule.conversion_reasons:
        return _skip_row(rule, "UNSUPPORTED_RUST_REGEX_SYNTAX")
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type not in GENERATABLE_TYPES:
        return _skip_row(rule, f"UNSUPPORTED_RULE_TYPE:{rule.p21_rule_type}")
    domain = _domain(rule)
    if not domain:
        return _skip_row(rule, "NO_CONCRETE_DOMAIN")
    variable = _variable(rule)
    if not variable:
        return _skip_row(rule, "NO_TARGET_VARIABLE")
    check, check_error = _build_check(rule, domain, variable)
    if check_error or check is None:
        return _skip_row(rule, check_error or "CHECK_NOT_GENERATABLE")
    unsupported_regex = _unsupported_regex_in_check(check)
    if unsupported_regex:
        return _skip_row(rule, unsupported_regex)
    required_operators = _required_operators(rule_type) | _operators_in_check(check)
    missing_operator = _unsupported_operator(required_operators, allowed_operators)
    if missing_operator:
        return _skip_row(rule, f"OPERATOR_NOT_ALLOWED:{missing_operator}")

    rule_id = generated_rule_id(rule)
    rule_dir = root / "generated_rules" / rule_id
    ensure_dir(rule_dir)
    rule_payload = _rule_yml(rule, rule_id, domain, check)
    (rule_dir / "rule.yml").write_text(yaml.safe_dump(rule_payload, sort_keys=False, allow_unicode=True), encoding="utf-8")
    _write_manifest(rule_dir, rule, rule_id, warnings)
    _write_expected_results(rule_dir, rule_id, rule, domain, variable)
    _write_data_case(rule_dir, "positive", rule, domain, variable)
    _write_data_case(rule_dir, "negative", rule, domain, variable)
    return {
        "source_rule_key": rule.source_rule_key,
        "p21_rule_id": rule.p21_rule_id,
        "generated_rule_id": rule_id,
        "generation_status": "GENERATED",
        "skip_reason": "",
        "p21_rule_type": rule.p21_rule_type,
        "domain": domain,
        "variable": variable,
    }


def generate_rules(
    rules: list[CanonicalRule],
    out_dir: str | Path,
    allowed_operators: set[str],
    limit: int | None = None,
    include_fuzzy_candidates: bool = False,
) -> GenerationSummary:
    if limit is not None and limit < 0:
        raise CliUsageError("limit must be zero or greater")
    selected = rules[:limit] if limit is not None else rules
    root = Path(out_dir)
    ensure_dir(root / "reports")
    rows = [
        _generate_one(
            rule,
            root,
            allowed_operators,
            include_fuzzy_candidates=include_fuzzy_candidates,
        )
        for rule in selected
    ]
    write_csv(root / "reports" / "generation_summary.csv", rows, SUMMARY_FIELDS)
    (root / "reports" / "generation_summary.json").write_text(
        json.dumps(
            {
                "generated_count": sum(1 for row in rows if row["generation_status"] == "GENERATED"),
                "skipped_count": sum(1 for row in rows if row["generation_status"] == "SKIPPED"),
                "rows": rows,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return GenerationSummary(rows)


def operator_set_from_inventory_rows(rows: list[dict[str, object]]) -> set[str]:
    operators: set[str] = set()
    for row in rows:
        operator = row.get("operator")
        if operator:
            operator_name = str(operator)
            operators.add(operator_name)
            if operator_name == "non_empty":
                operators.add("is_not_empty")
            if operator_name == "empty":
                operators.add("is_empty")
            if operator_name == "not_matches_regex":
                operators.add("does_not_match_regex")
        raw_keys = row.get("raw_keys")
        if isinstance(raw_keys, list):
            operators.update(str(item) for item in raw_keys)
    return operators
