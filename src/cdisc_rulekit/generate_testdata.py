from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Any

import yaml

from .convert_core import build_rule_yml, generated_rule_id, primary_domain, primary_variable
from .io_utils import ensure_dir, split_semicolon_list, write_csv
from .map_rules import standard_key
from .models import CanonicalRule, GeneratedRuleManifest


def _env_product(rule: CanonicalRule) -> str:
    key = standard_key(rule.standard_name)
    return key or "UNKNOWN"


def _env_version(rule: CanonicalRule) -> str:
    return str(rule.standard_version or "").replace(".", "-")


def _variable_type(variable: str) -> tuple[str, int]:
    upper = variable.upper()
    if upper.endswith("SEQ") or upper.endswith("DY") or any(token in upper for token in ("AGE", "AVAL", "BASE", "CHG", "PCHG")):
        return "Num", 8
    if upper == "DOMAIN":
        return "Char", 2
    return "Char", 200


def _columns(rule: CanonicalRule) -> list[str]:
    domain = primary_domain(rule)
    columns = ["STUDYID", "DOMAIN", "USUBJID"]
    if domain != "DM":
        columns.append(f"{domain}SEQ")
    target = primary_variable(rule)
    if target not in columns:
        columns.append(target)
    for variable in rule.variables:
        upper = variable.upper()
        if upper not in columns:
            columns.append(upper)
    return columns


def _value_for(variable: str, domain: str, target: str, target_value: str) -> str:
    if variable == "STUDYID":
        return "STUDY1"
    if variable == "DOMAIN":
        return domain
    if variable == "USUBJID":
        return "SUBJ001"
    if variable.endswith("SEQ"):
        return "1"
    if variable == target:
        return target_value
    return "VALUE"


def _positive_negative_values(rule: CanonicalRule) -> tuple[str, str]:
    rule_type = (rule.p21_rule_type or "").upper()
    if rule_type == "REGEX":
        return "2020-01-01", "01JAN2020"
    if rule_type == "MATCH":
        terms = split_semicolon_list(rule.raw_condition.get("terms"))
        return (terms[0] if terms else "Y"), "BAD"
    if rule_type == "REQUIRED":
        return "VALUE", ""
    if rule_type == "FIND":
        return "VALUE", ""
    return "VALID", "INVALID"


def _write_env(path: Path, rule: CanonicalRule) -> None:
    path.write_text(
        f"PRODUCT={_env_product(rule)}\nVERSION={_env_version(rule)}\nUSE_CASE=PROD\n",
        encoding="utf-8",
    )


def _write_case_data(rule: CanonicalRule, data_dir: Path, target_value: str) -> str:
    ensure_dir(data_dir)
    domain = primary_domain(rule)
    dataset_file = f"{domain.lower()}.csv"
    target = primary_variable(rule)
    columns = _columns(rule)
    _write_env(data_dir / ".env", rule)
    write_csv(data_dir / "_datasets.csv", [{"Filename": domain.lower(), "Label": domain}], ["Filename", "Label"])
    variable_rows = []
    for column in columns:
        kind, length = _variable_type(column)
        variable_rows.append(
            {
                "dataset": domain,
                "variable": column,
                "label": column,
                "type": kind,
                "length": length,
            }
        )
    write_csv(data_dir / "_variables.csv", variable_rows, ["dataset", "variable", "label", "type", "length"])
    with (data_dir / dataset_file).open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=columns)
        writer.writeheader()
        writer.writerow({column: _value_for(column, domain, target, target_value) for column in columns})
    return dataset_file


def _expected_rows(rule: CanonicalRule) -> list[dict[str, Any]]:
    domain = primary_domain(rule)
    variable = primary_variable(rule)
    return [
        {
            "test_case": "positive/01",
            "case_type": "positive",
            "expected_issue_count": 0,
            "dataset": "",
            "row": "",
            "variable": "",
            "source_rule_id": rule.p21_rule_id or rule.source_rule_id,
            "generated_rule_id": generated_rule_id(rule),
            "message": "",
            "notes": "",
        },
        {
            "test_case": "negative/01",
            "case_type": "negative",
            "expected_issue_count": 1,
            "dataset": domain,
            "row": 1,
            "variable": variable,
            "source_rule_id": rule.p21_rule_id or rule.source_rule_id,
            "generated_rule_id": generated_rule_id(rule),
            "message": rule.message or "",
            "notes": "Intentional violation",
        },
    ]


def generate_rule_folder(rule: CanonicalRule, output_root: str | Path) -> GeneratedRuleManifest:
    rule_id = generated_rule_id(rule)
    out = Path(output_root) / rule_id
    ensure_dir(out)
    ensure_dir(out / "positive" / "01" / "results")
    ensure_dir(out / "negative" / "01" / "results")

    rule_yaml = build_rule_yml(rule)
    (out / "rule.yml").write_text(yaml.safe_dump(rule_yaml, sort_keys=False), encoding="utf-8")

    positive_value, negative_value = _positive_negative_values(rule)
    _write_case_data(rule, out / "positive" / "01" / "data", positive_value)
    _write_case_data(rule, out / "negative" / "01" / "data", negative_value)
    write_csv(
        out / "expected_results.csv",
        _expected_rows(rule),
        [
            "test_case",
            "case_type",
            "expected_issue_count",
            "dataset",
            "row",
            "variable",
            "source_rule_id",
            "generated_rule_id",
            "message",
            "notes",
        ],
    )

    generated_files = sorted(str(path.relative_to(out)) for path in out.rglob("*") if path.is_file())
    manifest = GeneratedRuleManifest(
        generated_rule_id=rule_id,
        source_rule_id=rule.p21_rule_id or rule.source_rule_id,
        source=rule.source,
        conversion_status=rule.conversion_status or "",
        output_dir=str(out),
        generated_files=generated_files,
        expected_positive_issues=0,
        expected_negative_issues=1,
        warnings=[],
    )
    (out / "manifest.json").write_text(
        json.dumps(manifest.to_dict(), ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return manifest
