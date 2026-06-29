from __future__ import annotations

import csv
from dataclasses import dataclass
from pathlib import Path

import yaml

from .io_utils import ensure_dir


@dataclass(frozen=True)
class StructureValidationResult:
    ok: bool
    issues: list[str]
    checked_rule_count: int


def _read_csv_rows(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _validate_data_dir(data_dir: Path, issues: list[str]) -> None:
    for required in [".env", "_datasets.csv", "_variables.csv"]:
        if not (data_dir / required).exists():
            issues.append(f"{data_dir}: missing {required}")
    if not (data_dir / "_datasets.csv").exists() or not (data_dir / "_variables.csv").exists():
        return

    dataset_rows = _read_csv_rows(data_dir / "_datasets.csv")
    variable_rows = _read_csv_rows(data_dir / "_variables.csv")
    variables_by_dataset: dict[str, list[str]] = {}
    for row in variable_rows:
        dataset = row.get("dataset") or row.get("Dataset") or ""
        variable = row.get("variable") or row.get("Variable") or ""
        if dataset and variable:
            variables_by_dataset.setdefault(dataset, []).append(variable)

    for row in dataset_rows:
        filename = row.get("Filename") or row.get("filename")
        if not filename:
            issues.append(f"{data_dir / '_datasets.csv'}: missing Filename value")
            continue
        dataset_path = data_dir / f"{filename}.csv"
        if not dataset_path.exists():
            issues.append(f"{data_dir}: missing dataset CSV {filename}.csv")
            continue
        with dataset_path.open(newline="", encoding="utf-8") as handle:
            reader = csv.reader(handle)
            try:
                header = next(reader)
            except StopIteration:
                issues.append(f"{dataset_path}: empty dataset CSV")
                continue
        expected = variables_by_dataset.get(filename)
        if expected is None:
            issues.append(f"{data_dir / '_variables.csv'}: no variables for dataset {filename}")
            continue
        if header != expected:
            issues.append(f"{dataset_path}: header does not match _variables.csv")


def validate_generated_rules(generated_rules_dir: str | Path) -> StructureValidationResult:
    root = Path(generated_rules_dir)
    issues: list[str] = []
    rule_dirs = sorted(path for path in root.iterdir() if path.is_dir()) if root.exists() else []
    if not root.exists():
        issues.append(f"{root}: generated rules directory does not exist")

    for rule_dir in rule_dirs:
        for required in ["rule.yml", "manifest.json", "expected_results.csv"]:
            if not (rule_dir / required).exists():
                issues.append(f"{rule_dir}: missing {required}")
        rule_path = rule_dir / "rule.yml"
        if rule_path.exists():
            try:
                loaded = yaml.safe_load(rule_path.read_text(encoding="utf-8")) or {}
            except Exception as error:  # noqa: BLE001 - structure report includes parser error.
                issues.append(f"{rule_path}: malformed YAML: {error}")
                loaded = {}
            for key in ["Core", "Check", "Outcome", "Scope"]:
                if key not in loaded:
                    issues.append(f"{rule_path}: missing {key}")

        for case_type in ["positive", "negative"]:
            data_dir = rule_dir / case_type / "01" / "data"
            if not data_dir.exists():
                issues.append(f"{rule_dir}: missing {case_type}/01/data")
                continue
            _validate_data_dir(data_dir, issues)

    return StructureValidationResult(ok=not issues, issues=issues, checked_rule_count=len(rule_dirs))


def write_structure_validation_report(
    out_dir: str | Path,
    result: StructureValidationResult,
) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    lines = [
        "# Structure Validation",
        "",
        f"- ok: `{str(result.ok).lower()}`",
        f"- checked rules: `{result.checked_rule_count}`",
        "",
        "## Issues",
        "",
    ]
    if result.issues:
        lines.extend(f"- {issue}" for issue in result.issues)
    else:
        lines.append("- None")
    lines.append("")
    (out / "structure_validation.md").write_text("\n".join(lines), encoding="utf-8")
