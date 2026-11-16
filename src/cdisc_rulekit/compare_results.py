from __future__ import annotations

import csv
import json
from collections import Counter
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
CLASSIFICATION_FIELDS = [
    "classification",
    "row_count",
    "rule_count",
    "description",
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
        return bool(self.rows) and self.fail_count == 0


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


@dataclass(frozen=True)
class _ParsedReport:
    issues: tuple[dict[str, object], ...]
    skipped_count: int


class _InvalidActualReport(ValueError):
    pass


def _records(value: object, field: str, *, execution: bool = False) -> list[dict[str, Any]]:
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise _InvalidActualReport(f"malformed_report: {field} must be a list of objects")
    if execution and not value:
        raise _InvalidActualReport(f"unexecuted_report: {field} contains no execution records")
    return value


def _identity(record: dict[str, Any], expected_rule_id: str, *, required: str | None = None) -> None:
    if required and required not in record:
        raise _InvalidActualReport(f"wrong_rule: missing {required}")
    for key in ("rule_id", "core_id"):
        if key in record:
            value = record[key]
            if not isinstance(value, str) or not value.strip() or value != expected_rule_id:
                raise _InvalidActualReport(f"wrong_rule: {key} must identify {expected_rule_id!r}")


def _structural_types(record: dict[str, Any]) -> None:
    for key in ("dataset", "domain", "usubjid", "USUBJID", "row", "seq", "SEQ"):
        value = record.get(key)
        allowed = (str, int) if key in {"row", "seq", "SEQ"} else (str,)
        if value is not None and (isinstance(value, bool) or not isinstance(value, allowed)):
            raise _InvalidActualReport(f"malformed_report: invalid {key} type")
    variables = record.get("variables")
    if variables is not None and not isinstance(variables, str):
        if not isinstance(variables, list) or any(not isinstance(item, str) for item in variables):
            raise _InvalidActualReport("malformed_report: variables must be strings")


def _error_count(value: object, *, csv_text: bool = False) -> int:
    if csv_text and isinstance(value, str) and value.isascii() and value.isdigit():
        try:
            value = int(value)
        except ValueError as error:
            raise _InvalidActualReport("malformed_report: invalid error_count") from error
    if type(value) is not int or value < 0:
        raise _InvalidActualReport("malformed_report: error_count must be a nonnegative integer")
    return value


def _execution_status(record: dict[str, Any], *, official: bool = False) -> str:
    field = "status" if official else "execution_status"
    value = record.get(field)
    statuses = (
        {"SUCCESS": "passed", "ISSUE REPORTED": "failed", "SKIPPED": "skipped"}
        if official else {"passed": "passed", "failed": "failed", "skipped": "skipped"}
    )
    if not isinstance(value, str) or value not in statuses:
        raise _InvalidActualReport(f"unexecuted_report: unsupported {field} {value!r}")
    return statuses[value]


def _validate_count_status(status: str, count: int) -> None:
    if (status == "failed") != (count > 0):
        raise _InvalidActualReport("inconsistent_report: execution status contradicts error_count")


def _actual_from_json(path: Path, *, expected_rule_id: str) -> _ParsedReport:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError, RecursionError) as error:
        raise _InvalidActualReport(f"malformed_report: cannot read JSON: {error}") from error
    if not isinstance(payload, dict):
        raise _InvalidActualReport("unknown_format: JSON report must be an object")
    if "datasets" in payload or "results_data" in payload:
        raise _InvalidActualReport("unknown_format: contributor fixture JSON is not a supported production report")
    official = "Issue_Details" in payload or "Rules_Report" in payload
    if official == ("results" in payload):
        raise _InvalidActualReport("unknown_format: expected exactly one supported report schema")
    issues: list[dict[str, object]] = []
    if official:
        executions = _records(payload.get("Rules_Report"), "Rules_Report", execution=True)
        errors = _records(payload.get("Issue_Details"), "Issue_Details")
        statuses = []
        for result in executions:
            _identity(result, expected_rule_id, required="core_id")
            _structural_types(result)
            statuses.append(_execution_status(result, official=True))
        for error in errors:
            _identity(error, expected_rule_id, required="core_id")
            _structural_types(error)
            issues.append(_issue_from_error(error))
        if bool(issues) != ("failed" in statuses):
            raise _InvalidActualReport("inconsistent_report: Issue_Details contradicts Rules_Report")
        return _ParsedReport(tuple(issues), statuses.count("skipped"))
    executions = _records(payload.get("results"), "results", execution=True)
    status_counts: Counter[str] = Counter()
    for result in executions:
        _identity(result, expected_rule_id, required="rule_id")
        _structural_types(result)
        status = _execution_status(result)
        count = _error_count(result.get("error_count"))
        errors = _records(result.get("errors"), "errors")
        _validate_count_status(status, count)
        if count != len(errors):
            raise _InvalidActualReport("inconsistent_report: error_count does not match errors")
        for error in errors:
            _identity(error, expected_rule_id)
            _structural_types(error)
            issues.append(_issue_from_error(error, result))
        status_counts[status] += 1
    if "summary" in payload:
        summary = payload["summary"]
        if not isinstance(summary, dict):
            raise _InvalidActualReport("malformed_report: summary must be an object")
        actual_summary = {
            "total_results": len(executions),
            "passed": status_counts["passed"],
            "failed": status_counts["failed"],
            "skipped": status_counts["skipped"],
            "error_count": len(issues),
        }
        for field, actual_count in actual_summary.items():
            if field not in summary:
                continue
            value = summary[field]
            if type(value) is not int or value < 0:
                raise _InvalidActualReport(f"malformed_report: summary {field} must be a nonnegative integer")
            if value != actual_count:
                raise _InvalidActualReport(f"inconsistent_report: summary {field} does not match parsed results")
    return _ParsedReport(tuple(issues), status_counts["skipped"])


def _actual_from_csv(path: Path, *, expected_rule_id: str) -> _ParsedReport:
    try:
        with path.open(newline="", encoding="utf-8") as handle:
            reader = csv.DictReader(handle, strict=True)
            headers = reader.fieldnames or []
            if (not {"rule_id", "execution_status", "error_count"}.issubset(headers)
                    or len(set(headers)) != len(headers) or any(not name.strip() for name in headers)):
                raise _InvalidActualReport("malformed_report: CSV requires unique rule_id/execution_status/error_count headers")
            rows = list(reader)
    except (OSError, UnicodeError, csv.Error) as error:
        raise _InvalidActualReport(f"malformed_report: cannot read CSV: {error}") from error
    if not rows:
        raise _InvalidActualReport("unexecuted_report: CSV contains no execution records")
    issues: list[dict[str, object]] = []
    skipped_count = 0
    counts: Counter[int] = Counter()
    for row in rows:
        if None in row or any(value is None for value in row.values()):
            raise _InvalidActualReport("malformed_report: CSV row width does not match headers")
        _identity(row, expected_rule_id, required="rule_id")
        status = _execution_status(row)
        count = _error_count(row["error_count"], csv_text=True)
        _validate_count_status(status, count)
        if count:
            issues.append(_issue_from_error(row))
            counts[count] += 1
        elif any(row.get(key) for key in ("row", "variables", "usubjid", "seq")):
            raise _InvalidActualReport("inconsistent_report: zero-count CSV row contains issue details")
        skipped_count += status == "skipped"
    # Counts repeat per issue. Without result ids this checks representability,
    # not actual grouping; issue datasets can differ within a result.
    if any(frequency % count for count, frequency in counts.items()):
        raise _InvalidActualReport("inconsistent_report: CSV error_count distribution is not representable")
    return _ParsedReport(tuple(issues), skipped_count)


def _actual_issues(actual_dir: Path, *, expected_rule_id: str) -> _ParsedReport:
    report_json = actual_dir / "report.json"
    report_csv = actual_dir / "report.csv"
    if report_json.exists():
        return _actual_from_json(report_json, expected_rule_id=expected_rule_id)
    if report_csv.exists():
        return _actual_from_csv(report_csv, expected_rule_id=expected_rule_id)
    raise FileNotFoundError(f"missing report.json/report.csv in {actual_dir}")


def _matches_expected(
    issue: dict[str, object],
    expected: dict[str, str],
    *,
    strict_structure: bool = False,
) -> bool:
    if expected.get("rule_id") and issue.get("rule_id") != expected["rule_id"]:
        return False
    if expected.get("dataset") and issue.get("dataset") != expected["dataset"]:
        return False
    if expected.get("row") and str(issue.get("row") or "") != expected["row"]:
        return False
    expected_variables = _variables(expected.get("variables"))
    actual_variables = _variables(issue.get("variables"))
    if expected_variables:
        if strict_structure and expected_variables != actual_variables:
            return False
        if not strict_structure and not set(expected_variables).issubset(set(actual_variables)):
            return False
    if expected.get("usubjid") and issue.get("usubjid") != expected["usubjid"]:
        return False
    if expected.get("seq") and str(issue.get("seq") or "") != expected["seq"]:
        return False
    return True


def _issue_signature_from_issue(issue: dict[str, object]) -> tuple[str, str, str, tuple[str, ...], str, str]:
    return (
        str(issue.get("rule_id") or ""),
        str(issue.get("dataset") or ""),
        str(issue.get("row") or ""),
        tuple(_variables(issue.get("variables"))),
        str(issue.get("usubjid") or ""),
        str(issue.get("seq") or ""),
    )


def _issue_signature_from_expected(expected: dict[str, str], rule_id: str) -> tuple[str, str, str, tuple[str, ...], str, str]:
    return (
        expected.get("rule_id") or rule_id,
        expected.get("dataset") or "",
        expected.get("row") or "",
        tuple(_variables(expected.get("variables"))),
        expected.get("usubjid") or "",
        expected.get("seq") or "",
    )


def _compare_issue_index_group(
    rule_id: str,
    case_type: str,
    case_id: str,
    expected_rows: list[dict[str, str]],
    actual_root: Path,
) -> dict[str, object]:
    # issue_index rows are complete structural manifests, so this path always
    # compares exact signatures. strict_structure only controls legacy rows.
    first = expected_rows[0]
    expected_count = int(first.get("expected_issue_count") or 0)
    actual_dir = actual_root / rule_id / case_type / case_id
    issue_rows = [row for row in expected_rows if row.get("issue_index")]
    base = {
        "generated_rule_id": rule_id,
        "case_type": case_type,
        "case_id": case_id,
        "expected_issue_count": expected_count,
        "rule_id": first.get("rule_id") or rule_id,
        "dataset": first.get("dataset") or "",
        "row": first.get("row") or "",
        "variables": first.get("variables") or "",
        "usubjid": first.get("usubjid") or "",
        "seq": first.get("seq") or "",
    }
    if any((row.get("rule_id") or rule_id) != base["rule_id"] for row in expected_rows):
        return {**base, "actual_issue_count": "", "status": "EXPECTED_INVALID", "notes": "conflicting expected rule identities"}
    try:
        report = _actual_issues(actual_dir, expected_rule_id=base["rule_id"])
    except FileNotFoundError as error:
        return {**base, "actual_issue_count": "", "status": "ACTUAL_MISSING", "notes": str(error)}
    except _InvalidActualReport as error:
        return {**base, "actual_issue_count": "", "status": "ACTUAL_INVALID_REPORT", "notes": str(error)}
    issues, skipped_count = report.issues, report.skipped_count
    if skipped_count and issues:
        return {
            **base,
            "actual_issue_count": len(issues),
            "status": "ACTUAL_MIXED_SKIPPED_AND_ISSUES",
            "notes": "actual CORE output contains skipped result(s) and issue(s)",
        }
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
    if len(issue_rows) != expected_count:
        return {
            **base,
            "actual_issue_count": actual_count,
            "status": "FAIL",
            "notes": "issue_index rows do not match expected_issue_count",
        }
    expected_counts = Counter(_issue_signature_from_expected(row, rule_id) for row in issue_rows)
    actual_counts = Counter(_issue_signature_from_issue(issue) for issue in issues)
    if expected_counts == actual_counts:
        return {**base, "actual_issue_count": actual_count, "status": "PASS", "notes": ""}
    return {**base, "actual_issue_count": actual_count, "status": "FAIL", "notes": "structural issue fields did not match"}


def _compare_row(
    rule_id: str,
    expected: dict[str, str],
    actual_root: Path,
    *,
    strict_structure: bool = False,
) -> dict[str, object]:
    case_type = expected["case_type"]
    case_id = expected["case_id"]
    expected_count = int(expected.get("expected_issue_count") or 0)
    actual_dir = actual_root / rule_id / case_type / case_id
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
    try:
        report = _actual_issues(actual_dir, expected_rule_id=base["rule_id"])
    except FileNotFoundError as error:
        return {**base, "actual_issue_count": "", "status": "ACTUAL_MISSING", "notes": str(error)}
    except _InvalidActualReport as error:
        return {**base, "actual_issue_count": "", "status": "ACTUAL_INVALID_REPORT", "notes": str(error)}
    issues, skipped_count = report.issues, report.skipped_count
    if skipped_count and issues:
        return {
            **base,
            "actual_issue_count": len(issues),
            "status": "ACTUAL_MIXED_SKIPPED_AND_ISSUES",
            "notes": "actual CORE output contains skipped result(s) and issue(s)",
        }
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
    if any(
        _matches_expected(issue, expected, strict_structure=strict_structure)
        for issue in issues
    ):
        if expected_count > 1:
            return {
                **base,
                "actual_issue_count": actual_count,
                "status": "PARTIAL_STRUCTURAL_CHECK",
                "notes": "expected_results.csv only describes one representative issue for a multi-issue case",
            }
        return {**base, "actual_issue_count": actual_count, "status": "PASS", "notes": ""}
    return {**base, "actual_issue_count": actual_count, "status": "FAIL", "notes": "structural issue fields did not match"}


def _int_value(value: object) -> int:
    try:
        return int(value or 0)
    except (TypeError, ValueError):
        return 0


def _failure_classification(row: dict[str, object]) -> str:
    status = str(row.get("status") or "")
    notes = str(row.get("notes") or "")
    if status == "PASS":
        return "PASS"
    if status == "ACTUAL_MIXED_SKIPPED_AND_ISSUES":
        return "ACTUAL_MIXED_SKIPPED_AND_ISSUES"
    if status == "ACTUAL_SKIPPED":
        return "ACTUAL_SKIPPED_BY_CORE"
    if status == "ACTUAL_MISSING":
        return "ACTUAL_OUTPUT_MISSING"
    if status == "PARTIAL_STRUCTURAL_CHECK":
        return "PARTIAL_STRUCTURAL_CHECK"
    if status == "EXPECTED_MISSING":
        return "EXPECTED_OUTPUT_MISSING"
    if status == "EXPECTED_EMPTY":
        return "EXPECTED_OUTPUT_EMPTY"
    if status != "FAIL":
        return status or "UNKNOWN_NON_PASS"

    expected_count = _int_value(row.get("expected_issue_count"))
    actual_count = _int_value(row.get("actual_issue_count"))
    if notes == "issue count mismatch":
        if expected_count > 0 and actual_count == 0:
            return "NEGATIVE_DID_NOT_TRIGGER"
        if expected_count == 0 and actual_count > 0:
            return "POSITIVE_TRIGGERED_UNEXPECTEDLY"
        return "ISSUE_COUNT_MISMATCH"
    if notes == "structural issue fields did not match":
        return "STRUCTURAL_MISMATCH"
    return "SUPPORTED_MISMATCH"


def _classification_description(classification: str) -> str:
    descriptions = {
        "PASS": "Expected and actual structural fields matched.",
        "ACTUAL_MIXED_SKIPPED_AND_ISSUES": (
            "Actual CORE output mixed skipped execution with issue rows; this is a correctness failure."
        ),
        "ACTUAL_SKIPPED_BY_CORE": "Official CORE skipped execution; coverage gap, not a correctness mismatch.",
        "ACTUAL_OUTPUT_MISSING": "No actual CORE report was found for the generated case.",
        "ACTUAL_INVALID_REPORT": "Actual report lacks valid, consistent execution evidence for the requested rule.",
        "EXPECTED_INVALID": "Expected issue rows contain conflicting rule identities.",
        "EXPECTED_OUTPUT_MISSING": "The generated rule is missing expected_results.csv.",
        "EXPECTED_OUTPUT_EMPTY": "The generated rule has an empty expected_results.csv.",
        "NEGATIVE_DID_NOT_TRIGGER": "Negative generated data expected an issue but official CORE reported none.",
        "POSITIVE_TRIGGERED_UNEXPECTEDLY": "Positive generated data expected no issue but official CORE reported one.",
        "ISSUE_COUNT_MISMATCH": "Expected and actual issue counts differ.",
        "STRUCTURAL_MISMATCH": "Issue count matched but structural fields did not match.",
        "PARTIAL_STRUCTURAL_CHECK": (
            "Issue count matched, but expected_results.csv only described one representative issue."
        ),
        "SUPPORTED_MISMATCH": "Supported execution produced an unmatched result.",
    }
    return descriptions.get(classification, "Non-pass result requiring review.")


def _classification_summary(rows: list[dict[str, object]]) -> list[dict[str, object]]:
    classifications = sorted({_failure_classification(row) for row in rows})
    summary = []
    for classification in classifications:
        matching = [row for row in rows if _failure_classification(row) == classification]
        summary.append(
            {
                "classification": classification,
                "row_count": len(matching),
                "rule_count": len({str(row.get("generated_rule_id") or "") for row in matching}),
                "description": _classification_description(classification),
            },
        )
    return summary


def classification_counts(result: ComparisonResult) -> dict[str, int]:
    summary = _classification_summary(result.rows)
    return {str(row["classification"]): int(row["row_count"]) for row in summary}


def comparison_gate_ok(result: ComparisonResult, *, allow_actual_skipped: bool = False) -> bool:
    if result.ok:
        return True
    if not result.rows:
        return False
    if not allow_actual_skipped:
        return False
    return all(_failure_classification(row) in {"PASS", "ACTUAL_SKIPPED_BY_CORE"} for row in result.rows)


def compare_generated_results(
    generated_rules_dir: str | Path,
    actual_root: str | Path,
    *,
    strict_structure: bool = False,
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
        expected_rows = _read_csv(expected_path)
        if not expected_rows:
            rows.append(
                {
                    "generated_rule_id": rule_dir.name,
                    "case_type": "",
                    "case_id": "",
                    "expected_issue_count": "",
                    "actual_issue_count": "",
                    "status": "EXPECTED_EMPTY",
                    "rule_id": rule_dir.name,
                    "dataset": "",
                    "row": "",
                    "variables": "",
                    "usubjid": "",
                    "seq": "",
                    "notes": f"{expected_path} contains no expected rows",
                },
            )
            continue
        if expected_rows and "issue_index" in expected_rows[0]:
            grouped: dict[tuple[str, str], list[dict[str, str]]] = {}
            for expected in expected_rows:
                grouped.setdefault((expected["case_type"], expected["case_id"]), []).append(expected)
            for (case_type, case_id), group in sorted(grouped.items()):
                rows.append(_compare_issue_index_group(rule_dir.name, case_type, case_id, group, actual_root_path))
            continue
        for expected in expected_rows:
            rows.append(
                _compare_row(
                    rule_dir.name,
                    expected,
                    actual_root_path,
                    strict_structure=strict_structure,
                ),
            )
    return ComparisonResult(rows=rows)


def write_comparison_report(
    out_dir: str | Path,
    result: ComparisonResult,
    *,
    allow_actual_skipped: bool = False,
) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    write_csv(out / "comparison_summary.csv", result.rows, COMPARISON_FIELDS)
    classification_summary = _classification_summary(result.rows)
    supported_mismatch_classes = {
        "NEGATIVE_DID_NOT_TRIGGER",
        "POSITIVE_TRIGGERED_UNEXPECTEDLY",
        "ISSUE_COUNT_MISMATCH",
        "STRUCTURAL_MISMATCH",
        "SUPPORTED_MISMATCH",
    }
    supported_mismatch_rows = sum(
        int(row["row_count"])
        for row in classification_summary
        if row["classification"] in supported_mismatch_classes
    )
    coverage_gap_rows = sum(
        int(row["row_count"])
        for row in classification_summary
        if row["classification"] == "ACTUAL_SKIPPED_BY_CORE"
    )
    missing_output_rows = sum(
        int(row["row_count"])
        for row in classification_summary
        if row["classification"]
        in {"ACTUAL_OUTPUT_MISSING", "EXPECTED_OUTPUT_MISSING", "EXPECTED_OUTPUT_EMPTY"}
    )
    gate_ok = comparison_gate_ok(result, allow_actual_skipped=allow_actual_skipped)
    (out / "comparison_summary.json").write_text(
        json.dumps(
            {
                "allow_actual_skipped": allow_actual_skipped,
                "gate_ok": gate_ok,
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
    write_csv(out / "official_core_failure_classification.csv", classification_summary, CLASSIFICATION_FIELDS)
    (out / "official_core_failure_classification.json").write_text(
        json.dumps(
            {
                "row_count": len(result.rows),
                "pass_count": result.pass_count,
                "non_pass_count": result.fail_count,
                "supported_mismatch_rows": supported_mismatch_rows,
                "coverage_gap_rows": coverage_gap_rows,
                "missing_output_rows": missing_output_rows,
                "classifications": classification_summary,
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
        f"- gate ok: `{str(gate_ok).lower()}`",
        f"- allow actual skipped: `{str(allow_actual_skipped).lower()}`",
        f"- passed rows: `{result.pass_count}`",
        f"- failed rows: `{result.fail_count}`",
        f"- supported mismatch rows: `{supported_mismatch_rows}`",
        f"- skipped coverage-gap rows: `{coverage_gap_rows}`",
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

    classification_lines = [
        "# Official CORE Failure Classification",
        "",
        f"- total rows: `{len(result.rows)}`",
        f"- passed rows: `{result.pass_count}`",
        f"- non-pass rows: `{result.fail_count}`",
        f"- supported mismatch rows: `{supported_mismatch_rows}`",
        f"- skipped coverage-gap rows: `{coverage_gap_rows}`",
        f"- missing output rows: `{missing_output_rows}`",
        "",
        "## Summary",
        "",
    ]
    for row in classification_summary:
        classification_lines.append(
            f"- `{row['classification']}`: {row['row_count']} rows, {row['rule_count']} rules"
        )
    classification_lines.append("")
    (out / "official_core_failure_classification.md").write_text("\n".join(classification_lines), encoding="utf-8")
