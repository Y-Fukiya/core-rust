from __future__ import annotations

import csv
import json
import shutil
from dataclasses import dataclass
from pathlib import Path

from .io_utils import ensure_dir, write_csv

EXPORT_FIELDS = [
    "generated_rule_id",
    "export_status",
    "source_dir",
    "target_dir",
    "skip_reason",
]


@dataclass(frozen=True)
class ExportSummary:
    rows: list[dict[str, object]]

    @property
    def exported_count(self) -> int:
        return sum(1 for row in self.rows if row["export_status"] == "EXPORTED")

    @property
    def skipped_count(self) -> int:
        return sum(1 for row in self.rows if row["export_status"] == "SKIPPED")


def _copy_rule_dir(source: Path, target: Path, overwrite: bool) -> dict[str, object]:
    row = {
        "generated_rule_id": source.name,
        "source_dir": str(source),
        "target_dir": str(target),
    }
    if target.exists() and not overwrite:
        return {**row, "export_status": "SKIPPED", "skip_reason": "TARGET_EXISTS"}
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(source, target)
    return {**row, "export_status": "EXPORTED", "skip_reason": ""}


def _resolve_target_root(open_rules_repo: str | Path, target_subdir: str | Path) -> Path:
    repo_root = Path(open_rules_repo).resolve()
    if not repo_root.is_dir():
        raise ValueError(f"{repo_root}: open_rules_repo does not exist or is not a directory")
    subdir = Path(target_subdir)
    if subdir.is_absolute() or ".." in subdir.parts:
        raise ValueError("target_subdir must be a relative path inside open_rules_repo")
    target_root = (repo_root / subdir).resolve()
    if not target_root.is_relative_to(repo_root):
        raise ValueError("target_subdir must resolve inside open_rules_repo")
    return target_root


def _skip_rule_dir(source: Path, target: Path, reason: str) -> dict[str, object]:
    return {
        "generated_rule_id": source.name,
        "source_dir": str(source),
        "target_dir": str(target),
        "export_status": "SKIPPED",
        "skip_reason": reason,
    }


def _comparison_passed_rule_ids(path: Path) -> set[str]:
    statuses_by_rule: dict[str, list[str]] = {}
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rule_id = row.get("generated_rule_id") or ""
            if not rule_id:
                continue
            statuses_by_rule.setdefault(rule_id, []).append(row.get("status") or "")
    return {
        rule_id
        for rule_id, statuses in statuses_by_rule.items()
        if statuses and all(status == "PASS" for status in statuses)
    }


def export_generated_rules(
    generated_rules_dir: str | Path,
    open_rules_repo: str | Path,
    target_subdir: str | Path = "Unpublished/NEW-RULE",
    overwrite: bool = False,
    comparison_summary: str | Path | None = None,
    only_passed: bool = False,
) -> ExportSummary:
    generated_root = Path(generated_rules_dir)
    if not generated_root.exists():
        raise ValueError(f"{generated_root}: generated rules directory does not exist")
    rule_dirs = sorted(path for path in generated_root.iterdir() if path.is_dir())
    if not rule_dirs:
        raise ValueError(f"{generated_root}: no generated rule directories found")
    if only_passed and comparison_summary is None:
        raise ValueError("comparison_summary is required when only_passed is true")
    passed_rule_ids = _comparison_passed_rule_ids(Path(comparison_summary)) if only_passed else None
    target_root = _resolve_target_root(open_rules_repo, target_subdir)
    ensure_dir(target_root)
    rows = [
        _copy_rule_dir(rule_dir, target_root / rule_dir.name, overwrite)
        if passed_rule_ids is None or rule_dir.name in passed_rule_ids
        else _skip_rule_dir(rule_dir, target_root / rule_dir.name, "COMPARISON_NOT_PASS")
        for rule_dir in rule_dirs
    ]
    summary = ExportSummary(rows)
    write_csv(target_root / "export_manifest.csv", rows, EXPORT_FIELDS)
    (target_root / "export_manifest.json").write_text(
        json.dumps(
            {
                "target_root": str(target_root),
                "exported_count": summary.exported_count,
                "skipped_count": summary.skipped_count,
                "rows": rows,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return summary
