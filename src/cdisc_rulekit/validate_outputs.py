from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Any

import yaml

from .io_utils import ensure_dir


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _validate_case(rule_dir: Path, case_type: str, errors: list[str]) -> None:
    data_dir = rule_dir / case_type / "01" / "data"
    results_dir = rule_dir / case_type / "01" / "results"
    required = [".env", "_datasets.csv", "_variables.csv"]
    for name in required:
        if not (data_dir / name).exists():
            errors.append(f"missing {case_type}/01/data/{name}")
    if not results_dir.is_dir():
        errors.append(f"missing {case_type}/01/results")
    if errors:
        return

    datasets = _read_csv(data_dir / "_datasets.csv")
    variables = _read_csv(data_dir / "_variables.csv")
    for dataset in datasets:
        filename = dataset.get("Filename", "")
        csv_path = data_dir / f"{filename}.csv"
        if not csv_path.exists():
            errors.append(f"missing {case_type}/01/data/{filename}.csv")
            continue
        with csv_path.open(newline="", encoding="utf-8") as handle:
            reader = csv.DictReader(handle)
            columns = reader.fieldnames or []
        metadata_columns = [row["variable"] for row in variables if row.get("dataset", "").upper() == filename.upper()]
        if len(columns) != len(set(columns)):
            errors.append(f"duplicate columns in {case_type}/01/data/{filename}.csv")
        if len(metadata_columns) != len(set(metadata_columns)):
            errors.append(f"duplicate variables in {case_type}/01/data/_variables.csv for {filename.upper()}")
        if columns != metadata_columns:
            errors.append(f"dataset columns do not match _variables.csv for {case_type}/01/data/{filename}.csv")


def validate_rule_folder(rule_dir: Path) -> dict[str, Any]:
    errors: list[str] = []
    rule_yml = rule_dir / "rule.yml"
    manifest = rule_dir / "manifest.json"
    expected = rule_dir / "expected_results.csv"
    for path in (rule_yml, manifest, expected):
        if not path.exists():
            errors.append(f"missing {path.name}")

    if rule_yml.exists():
        try:
            data = yaml.safe_load(rule_yml.read_text(encoding="utf-8")) or {}
        except Exception as error:  # noqa: BLE001
            errors.append(f"invalid rule.yml: {error}")
            data = {}
        if data.get("Core", {}).get("Id") != rule_dir.name:
            errors.append("Core.Id does not match generated folder id")
        if not data.get("Outcome", {}).get("Message"):
            errors.append("Outcome.Message is empty")
        if "Check" not in data:
            errors.append("Check is missing")

    _validate_case(rule_dir, "positive", errors)
    _validate_case(rule_dir, "negative", errors)
    return {"rule_id": rule_dir.name, "valid": not errors, "errors": errors}


def validate_generated_rules(generated_root: str | Path) -> dict[str, Any]:
    root = Path(generated_root)
    rules = []
    if root.exists():
        for rule_dir in sorted(path for path in root.iterdir() if path.is_dir()):
            rules.append(validate_rule_folder(rule_dir))
    failed = sum(1 for item in rules if not item["valid"])
    return {"summary": {"total": len(rules), "failed": failed}, "rules": rules}


def write_structure_validation(report: dict[str, Any], out_dir: str | Path) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    (out / "structure_validation.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    lines = ["# Structure Validation", "", f"- Total: {report['summary']['total']}", f"- Failed: {report['summary']['failed']}", ""]
    for rule in report["rules"]:
        status = "PASS" if rule["valid"] else "FAIL"
        lines.append(f"## {rule['rule_id']} - {status}")
        if rule["errors"]:
            lines.extend(f"- {error}" for error in rule["errors"])
        else:
            lines.append("- No structural errors")
        lines.append("")
    (out / "structure_validation.md").write_text("\n".join(lines), encoding="utf-8")
