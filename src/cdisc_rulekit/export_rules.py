from __future__ import annotations

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


def export_generated_rules(
    generated_rules_dir: str | Path,
    open_rules_repo: str | Path,
    target_subdir: str | Path = "Unpublished/NEW-RULE",
    overwrite: bool = False,
) -> ExportSummary:
    generated_root = Path(generated_rules_dir)
    target_root = Path(open_rules_repo) / Path(target_subdir)
    ensure_dir(target_root)
    rule_dirs = sorted(path for path in generated_root.iterdir() if path.is_dir()) if generated_root.exists() else []
    rows = [_copy_rule_dir(rule_dir, target_root / rule_dir.name, overwrite) for rule_dir in rule_dirs]
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
