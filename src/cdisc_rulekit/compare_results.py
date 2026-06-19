from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .io_utils import ensure_dir, write_csv

COMPARISON_FIELDS = [
    "generated_rule_id",
    "case_type",
    "case_id",
    "expected_issue_count",
    "actual_issue_count",
    "status",
    "rule_id",
    "dataset",
    "row",
    "variables",
    "usubjid",
    "seq",
    "notes",
]


@dataclass(frozen=True)
class ComparisonResult:
    rows: list[dict[str, object]]

    @property
    def pass_count(self) -> int:
        return sum(1 for row in self.rows if row["status"] == "PASS")

    @property
    def fail_count(self) -> int:
        return len(self.rows) - self.pass_count

    @property
    def ok(self) -> bool:
        return self.fail_count == 0


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _variables(value: object) -> list[str]:
    if value is None:
        return []
    if isinstance(value, list):
        return sorted(str(item) for item in value if str(item))
    text = str(value)
    if not text:
        return []
    if "|" in text:
        return sorted(part for part in text.split("|") if part)
    return [text]


def _issue_from_error(error: dict[str, Any], fallback: dict[str, Any] | None = None) -> dict[str, object]:
    fallback = fallback or {}
    return {
        "rule_id": error.get("rule_id")
        or error.get("core_id")
        or fallback.get("rule_id")
        or fallback.get("core_id")
        or "",
        "dataset": error.get("dataset") or error.get("domain") or fallback.get("dataset") or fallback.get("domain") or "",
        "row": str(error.get("row") or ""),
        "variables": _variables(error.get("variables") or fallback.get("variables")),
        "usubjid": error.get("usubjid")
        or error.get("USUBJID")
        or fallback.get("usubjid")
        or fallback.get("USUBJID")
        or "",
        "seq": str(error.get("seq") or error.get("SEQ") or fallback.get("seq") or fallback.get("SEQ") or ""),
    }


def _actual_from_json(path: Path) -> tuple[list[dict[str, object]], int]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    issues: list[dict[str, object]] = []
    skipped_count = 0
    if "Issue_Details" in payload or "Rules_Report" in payload:
        for result in payload.get("Rules_Report") or []:
            if str(result.get("status") or "").lower() == "skipped":
                skipped_count += 1
        for error in payload.get("Issue_Details") or []:
            if isinstance(error, dict):
                issues.append(_issue_from_error(error))
        return issues, skipped_count
    for result in payload.get("results") or []:
        if str(result.get("execution_status") or "").lower() == "skipped":
            skipped_count += 1
            continue
        errors = result.get("errors") or []
        if errors:
            for error in errors:
                if isinstance(error, dict):
                    issues.append(_issue_from_error(error, result))
            continue
        error_count = int(result.get("error_count") or 0)
        if error_count:
            issues.append(_issue_from_error(result))
    return issues, skipped_count


def _actual_from_csv(path: Path) -> tuple[list[dict[str, object]], int]:
    issues: list[dict[str, object]] = []
    skipped_count = 0
    for row in _read_csv(path):
        if str(row.get("execution_status") or "").lower() == "skipped":
            skipped_count += 1
            continue
        if int(row.get("error_count") or 0) <= 0:
            continue
        issues.append(
            {
                "rule_id": row.get("rule_id") or "",
                "dataset": row.get("dataset") or row.get("domain") or "",
                "row": row.get("row") or "",
                "variables": _variables(row.get("variables")),
                "usubjid": row.get("usubjid") or "",
                "seq": row.get("seq") or "",
            },
        )
    return issues, skipped_count


def _actual_issues(actual_dir: Path) -> tuple[list[dict[str, object]] | None, str, int]:
    report_json = actual_dir / "report.json"
    report_csv = actual_dir / "report.csv"
    if report_json.exists():
        issues, skipped_count = _actual_from_json(report_json)
        return issues, "", skipped_count
    if report_csv.exists():
        issues, skipped_count = _actual_from_csv(report_csv)
        return issues, "", skipped_count
    return None, f"missing report.json/report.csv in {actual_dir}", 0


def _matches_expected(issue: dict[str, object], expected: dict[str, str]) -> bool:
    if expected.get("rule_id") and issue.get("rule_id") != expected["rule_id"]:
        return False
    if expected.get("dataset") and issue.get("dataset") != expected["dataset"]:
        return False
    if expected.get("row") and str(issue.get("row") or "") != expected["row"]:
        return False
    expected_variables = _variables(expected.get("variables"))
    if expected_variables and _variables(issue.get("variables")) != expected_variables:
        return False
    if expected.get("usubjid") and issue.get("usubjid") != expected["usubjid"]:
        return False
    if expected.get("seq") and str(issue.get("seq") or "") != expected["seq"]:
        return False
    return True


def _compare_row(rule_id: str, expected: dict[str, str], actual_root: Path) -> dict[str, object]:
    case_type = expected["case_type"]
    case_id = expected["case_id"]
    expected_count = int(expected.get("expected_issue_count") or 0)
    actual_dir = actual_root / rule_id / case_type / case_id
    issues, missing_note, skipped_count = _actual_issues(actual_dir)
    base = {
        "generated_rule_id": rule_id,
        "case_type": case_type,
        "case_id": case_id,
        "expected_issue_count": expected_count,
        "rule_id": expected.get("rule_id") or rule_id,
        "dataset": expected.get("dataset") or "",
        "row": expected.get("row") or "",
        "variables": expected.get("variables") or "",
        "usubjid": expected.get("usubjid") or "",
        "seq": expected.get("seq") or "",
    }
    if issues is None:
        return {**base, "actual_issue_count": "", "status": "ACTUAL_MISSING", "notes": missing_note}
    if skipped_count:
        return {
            **base,
            "actual_issue_count": len(issues),
            "status": "ACTUAL_SKIPPED",
            "notes": "actual CORE output contains skipped result(s)",
        }
    actual_count = len(issues)
    if expected_count != actual_count:
        return {**base, "actual_issue_count": actual_count, "status": "FAIL", "notes": "issue count mismatch"}
    if expected_count == 0:
        return {**base, "actual_issue_count": actual_count, "status": "PASS", "notes": ""}
    if any(_matches_expected(issue, expected) for issue in issues):
        return {**base, "actual_issue_count": actual_count, "status": "PASS", "notes": ""}
    return {**base, "actual_issue_count": actual_count, "status": "FAIL", "notes": "structural issue fields did not match"}


def compare_generated_results(
    generated_rules_dir: str | Path,
    actual_root: str | Path,
) -> ComparisonResult:
    generated_root = Path(generated_rules_dir)
    actual_root_path = Path(actual_root)
    rows: list[dict[str, object]] = []
    rule_dirs = sorted(path for path in generated_root.iterdir() if path.is_dir()) if generated_root.exists() else []
    for rule_dir in rule_dirs:
        expected_path = rule_dir / "expected_results.csv"
        if not expected_path.exists():
            rows.append(
                {
                    "generated_rule_id": rule_dir.name,
                    "case_type": "",
                    "case_id": "",
                    "expected_issue_count": "",
                    "actual_issue_count": "",
                    "status": "EXPECTED_MISSING",
                    "rule_id": rule_dir.name,
                    "dataset": "",
                    "row": "",
                    "variables": "",
                    "usubjid": "",
                    "seq": "",
                    "notes": f"missing {expected_path}",
                },
            )
            continue
        for expected in _read_csv(expected_path):
            rows.append(_compare_row(rule_dir.name, expected, actual_root_path))
    return ComparisonResult(rows=rows)


def write_comparison_report(out_dir: str | Path, result: ComparisonResult) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    write_csv(out / "comparison_summary.csv", result.rows, COMPARISON_FIELDS)
    (out / "comparison_summary.json").write_text(
        json.dumps(
            {
                "ok": result.ok,
                "pass_count": result.pass_count,
                "fail_count": result.fail_count,
                "rows": result.rows,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    lines = [
        "# Comparison Summary",
        "",
        f"- ok: `{str(result.ok).lower()}`",
        f"- passed rows: `{result.pass_count}`",
        f"- failed rows: `{result.fail_count}`",
        "",
        "## Failures",
        "",
    ]
    failures = [row for row in result.rows if row["status"] != "PASS"]
    if failures:
        lines.extend(
            f"- `{row['generated_rule_id']}` `{row['case_type']}/{row['case_id']}`: {row['status']} {row['notes']}"
            for row in failures
        )
    else:
        lines.append("- None")
    lines.append("")
    (out / "comparison_summary.md").write_text("\n".join(lines), encoding="utf-8")
