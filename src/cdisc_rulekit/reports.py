from __future__ import annotations

import json
from collections import Counter
from pathlib import Path

from .io_utils import ensure_dir
from .models import CanonicalRule, RuleMapping


def _status_counts(rules: list[CanonicalRule]) -> dict[str, int]:
    return dict(sorted(Counter(rule.conversion_status or "UNCLASSIFIED" for rule in rules).items()))


def _mapping_counts(mappings: list[RuleMapping]) -> dict[str, int]:
    return dict(sorted(Counter(mapping.match_type for mapping in mappings).items()))


def write_conversion_summary(
    report_dir: str | Path,
    classified_rules: list[CanonicalRule],
    warnings: list[str],
) -> None:
    out = Path(report_dir)
    ensure_dir(out)
    counts = _status_counts(classified_rules)
    lines = [
        "# Conversion Status Summary",
        "",
        "Read-only phase: no `rule.yml`, positive data, or negative data was generated.",
        "",
        "## Status Counts",
        "",
    ]
    for status, count in counts.items():
        lines.append(f"- `{status}`: {count}")
    lines.extend(["", "## Warnings", ""])
    if warnings:
        lines.extend(f"- {warning}" for warning in warnings)
    else:
        lines.append("- None")
    lines.append("")
    (out / "conversion_status_summary.md").write_text("\n".join(lines), encoding="utf-8")


def write_readiness_summary(
    report_dir: str | Path,
    classified_rules: list[CanonicalRule],
    mappings: list[RuleMapping],
    warnings: list[str],
) -> None:
    out = Path(report_dir)
    ensure_dir(out)
    payload = {
        "total_p21_rules": len(classified_rules),
        "status_counts": _status_counts(classified_rules),
        "mapping_counts": _mapping_counts(mappings),
        "generated_rules_created": 0,
        "warnings": warnings,
    }
    (out / "readiness_summary.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
