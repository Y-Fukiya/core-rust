from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from cdisc_rulekit.load_open_rules import load_open_rules  # noqa: E402
from cdisc_rulekit.map_rules import map_p21_to_core  # noqa: E402
from cdisc_rulekit.models import CanonicalRule  # noqa: E402

FIXTURE_ROOT = ROOT / "tests" / "cdisc_rulekit" / "fixtures"
P21_FIXTURE_ROOT = FIXTURE_ROOT / "p21"
P21PORT_FIXTURE_ROOT = FIXTURE_ROOT / "p21port"
SMOKE_STEP_TIMEOUT_SECONDS = 60


def _run(args: list[str], *, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
        text=True,
        timeout=SMOKE_STEP_TIMEOUT_SECONDS,
    )


def _run_expect_failure(args: list[str], *, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        env=env,
        check=False,
        capture_output=True,
        text=True,
        timeout=SMOKE_STEP_TIMEOUT_SECONDS,
    )
    if completed.returncode == 0:
        raise AssertionError(f"expected command to fail: {args!r}")
    return completed


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _json_array(value: str) -> list[str]:
    if not value:
        return []
    payload = json.loads(value)
    return [str(item) for item in payload]


def _mapping_rows(path: Path) -> list[dict[str, object]]:
    rows = []
    for row in _read_csv(path):
        rows.append(
            {
                "p21_rule_id": row["p21_rule_id"],
                "core_rule_id": row["core_rule_id"],
                "match_type": row["match_type"],
                "confidence": row["confidence"],
                "cdisc_rule_ids": ";".join(_json_array(row["cdisc_rule_id_overlap"])),
            },
        )
    return rows


def _baseline_mapping_rows() -> list[dict[str, str]]:
    return _read_csv(P21PORT_FIXTURE_ROOT / "p21_to_core_mapping_baseline.csv")


def _write_fake_engine(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "from __future__ import annotations",
                "import csv",
                "import json",
                "import pathlib",
                "import sys",
                "",
                "args = sys.argv[1:]",
                "output = pathlib.Path(args[args.index('--output') + 1])",
                "rule_path = pathlib.Path(args[args.index('--local-rules') + 1])",
                "rule_dir = rule_path.parent",
                "case_type = output.parent.name",
                "case_id = output.name",
                "expected_path = rule_dir / 'expected_results.csv'",
                "with expected_path.open(newline='', encoding='utf-8') as handle:",
                "    expected_rows = [row for row in csv.DictReader(handle) if row['case_type'] == case_type and row['case_id'] == case_id]",
                "errors = []",
                "issue_count = 0",
                "for row in expected_rows:",
                "    count = int(row['expected_issue_count'])",
                "    issue_count += count",
                "    if count <= 0:",
                "        continue",
                "    errors.append({",
                "        'rule_id': row['rule_id'],",
                "        'dataset': row['dataset'],",
                "        'row': int(row['row']) if row['row'] else '',",
                "        'variables': [part for part in row['variables'].split('|') if part],",
                "        'usubjid': row.get('usubjid') or '',",
                "        'seq': row.get('seq') or '',",
                "    })",
                "payload = {'summary': {'error_count': issue_count}, 'results': []}",
                "if errors:",
                "    payload['results'].append({'rule_id': errors[0]['rule_id'], 'error_count': issue_count, 'errors': errors})",
                "output.mkdir(parents=True, exist_ok=True)",
                "(output / 'report.json').write_text(json.dumps(payload, sort_keys=True), encoding='utf-8')",
                "(output / 'report.csv').write_text('rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\\n', encoding='utf-8')",
            ],
        )
        + "\n",
        encoding="utf-8",
    )


def _comparison_baseline() -> dict[str, object]:
    payload = json.loads((P21PORT_FIXTURE_ROOT / "comparison_summary_baseline.json").read_text(encoding="utf-8"))
    payload["rows"].sort(key=lambda row: (row["case_type"], row["case_id"], row["generated_rule_id"]))
    return payload


def _comparison_projection(summary: dict[str, object]) -> dict[str, object]:
    rows = []
    for row in summary["rows"]:
        rows.append(
            {
                "actual_issue_count": int(row["actual_issue_count"]),
                "case_id": row["case_id"],
                "case_type": row["case_type"],
                "dataset": row["dataset"],
                "expected_issue_count": int(row["expected_issue_count"]),
                "generated_rule_id": row["generated_rule_id"],
                "rule_id": row["rule_id"],
                "status": row["status"],
                "variables": row["variables"],
            },
        )
    rows.sort(key=lambda row: (row["case_type"], row["case_id"], row["generated_rule_id"]))
    return {
        "fail_count": summary["fail_count"],
        "gate_ok": summary["gate_ok"],
        "ok": summary["ok"],
        "pass_count": summary["pass_count"],
        "rows": rows,
    }


def _failure_case_projection(summary: dict[str, object]) -> list[dict[str, object]]:
    rows = []
    for row in summary["rows"]:
        if row["status"] != "FAIL":
            continue
        rows.append(
            {
                "actual_issue_count": int(row["actual_issue_count"]),
                "case_id": row["case_id"],
                "case_type": row["case_type"],
                "expected_issue_count": int(row["expected_issue_count"]),
                "generated_rule_id": row["generated_rule_id"],
                "notes": row["notes"],
                "status": row["status"],
                "variables": row["variables"],
            },
        )
    rows.sort(key=lambda row: (row["case_type"], row["case_id"], row["generated_rule_id"]))
    return rows


def _failure_direction_counts(failed_cases: list[dict[str, object]]) -> dict[str, int]:
    missing_issue = 0
    extra_issue = 0
    equal_count_mismatch = 0
    for row in failed_cases:
        actual = int(row["actual_issue_count"])
        expected = int(row["expected_issue_count"])
        if actual < expected:
            missing_issue += 1
        elif actual > expected:
            extra_issue += 1
        else:
            equal_count_mismatch += 1
    return {
        "equal_count_mismatch": equal_count_mismatch,
        "extra_issue": extra_issue,
        "missing_issue": missing_issue,
    }


def _write_empty_report(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    payload = {"summary": {"error_count": 0}, "results": []}
    (output / "report.json").write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")


def _write_extra_issue_report(output: Path, rule_id: str) -> None:
    output.mkdir(parents=True, exist_ok=True)
    errors = [
        {"rule_id": rule_id, "dataset": "AE", "row": 1, "variables": ["AETERM"]},
        {"rule_id": rule_id, "dataset": "AE", "row": 2, "variables": ["AETERM"]},
    ]
    payload = {
        "summary": {"error_count": len(errors)},
        "results": [{"rule_id": rule_id, "error_count": len(errors), "errors": errors}],
    }
    (output / "report.json").write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")


def _write_wrong_issue_report(output: Path, rule_id: str) -> None:
    output.mkdir(parents=True, exist_ok=True)
    errors = [{"rule_id": rule_id, "dataset": "CM", "row": 2, "variables": ["AETERM"]}]
    payload = {
        "summary": {"error_count": len(errors)},
        "results": [{"rule_id": rule_id, "error_count": len(errors), "errors": errors}],
    }
    (output / "report.json").write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")


def _write_failure_probe_actuals(generated_rules: Path, actual_root: Path) -> None:
    rule_dirs = sorted(path for path in generated_rules.iterdir() if path.is_dir())
    if not rule_dirs:
        raise AssertionError("expected at least one generated P21PORT rule")
    for index, rule_dir in enumerate(rule_dirs):
        rule_id = rule_dir.name
        _write_empty_report(actual_root / rule_id / "positive" / "01")
        if index == 0:
            _write_wrong_issue_report(actual_root / rule_id / "negative" / "01", rule_id)
        else:
            _write_extra_issue_report(actual_root / rule_id / "negative" / "01", rule_id)


def _fuzzy_mapping_probe() -> dict[str, object]:
    core_rules, _inventory, _warnings = load_open_rules(FIXTURE_ROOT / "open_rules")
    p21_rule = CanonicalRule(
        source="P21",
        source_rule_id="FUZZY1",
        p21_rule_id="FUZZY1",
        standard_name="SDTM-IG",
        p21_rule_type="Match",
        domains=["AE"],
        variables=["AETERM"],
        message="AETERM must be present.",
        description="AETERM must be populated.",
    )
    mapping = map_p21_to_core([p21_rule], core_rules)[0]
    return {
        "confidence_above_threshold": mapping.confidence >= 0.60,
        "match_type": mapping.match_type,
    }


def _duplicate_rule_id_probe() -> int:
    duplicate_rules = [
        CanonicalRule(
            source="P21",
            source_rule_id="DUP001",
            source_rule_key="config-a|DUP001",
            p21_rule_id="DUP001",
            standard_name="SDTM-IG",
            p21_rule_type="Required",
            domains=["DM"],
            variables=["USUBJID"],
            message="USUBJID is required.",
            description="USUBJID is required.",
        ),
        CanonicalRule(
            source="P21",
            source_rule_id="DUP001",
            source_rule_key="config-b|DUP001",
            p21_rule_id="DUP001",
            standard_name="SDTM-IG",
            p21_rule_type="Required",
            domains=["AE"],
            variables=["AETERM"],
            message="AETERM is required.",
            description="AETERM is required.",
        ),
    ]
    mappings = map_p21_to_core(duplicate_rules, [])
    return len({mapping.p21_rule_key for mapping in mappings})


def run(work_dir: Path, *, real_engine_command: str | None = None) -> dict[str, object]:
    work_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    existing_pythonpath = env.get("PYTHONPATH")
    env["PYTHONPATH"] = (
        f"{ROOT / 'src'}{os.pathsep}{existing_pythonpath}"
        if existing_pythonpath
        else str(ROOT / "src")
    )
    read_only = work_dir / "read_only"
    generated = work_dir / "generated"
    unsupported_probe = work_dir / "unsupported_probe"
    run_out = work_dir / "run"
    failure_probe = work_dir / "failure_probe"
    reports = work_dir / "reports"

    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "build-readonly",
            "--p21-rules",
            str(P21_FIXTURE_ROOT / "cdisc_rule_definitions_latest_2204.csv"),
            "--p21-domain-map",
            str(P21_FIXTURE_ROOT / "cdisc_rule_domain_map.csv"),
            "--open-rules-repo",
            str(FIXTURE_ROOT / "open_rules"),
            "--out",
            str(read_only),
            "--standard",
            "SDTM-IG",
        ],
        env=env,
    )
    mapping_rows = _mapping_rows(read_only / "mapping" / "p21_to_core_mapping.csv")
    baseline_mapping = _baseline_mapping_rows()
    if mapping_rows != baseline_mapping:
        raise AssertionError(f"P21 mapping baseline mismatch: {mapping_rows!r} != {baseline_mapping!r}")

    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "generate",
            "--p21-catalog",
            str(read_only / "catalog" / "p21_rules_normalized.jsonl"),
            "--conversion-status",
            str(read_only / "catalog" / "conversion_status.csv"),
            "--operator-inventory",
            str(P21PORT_FIXTURE_ROOT / "core_operator_inventory_for_generation.csv"),
            "--out",
            str(generated),
        ],
        env=env,
    )
    generation_summary = json.loads((generated / "reports" / "generation_summary.json").read_text(encoding="utf-8"))

    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "generate",
            "--p21-catalog",
            str(read_only / "catalog" / "p21_rules_normalized.jsonl"),
            "--conversion-status",
            str(read_only / "catalog" / "conversion_status.csv"),
            "--operator-inventory",
            str(read_only / "catalog" / "core_operator_inventory.csv"),
            "--out",
            str(unsupported_probe),
        ],
        env=env,
    )
    unsupported_generation_summary = json.loads(
        (unsupported_probe / "reports" / "generation_summary.json").read_text(encoding="utf-8"),
    )

    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "validate-structure",
            "--generated-rules",
            str(generated / "generated_rules"),
            "--out",
            str(reports),
        ],
        env=env,
    )

    fake_engine = work_dir / "fake_core_engine.py"
    _write_fake_engine(fake_engine)
    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "run-core",
            "--generated-rules",
            str(generated / "generated_rules"),
            "--out",
            str(run_out),
            "--engine-command",
            f"{sys.executable} {fake_engine}",
        ],
        env=env,
    )
    run_summary_rows = _read_csv(run_out / "reports" / "core_run_execution_summary.csv")

    _run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "compare-results",
            "--generated-rules",
            str(generated / "generated_rules"),
            "--actual-root",
            str(run_out / "core_runs"),
            "--out",
            str(reports),
            "--strict-structure",
        ],
        env=env,
    )
    comparison_summary = json.loads((reports / "comparison_summary.json").read_text(encoding="utf-8"))
    comparison_projection = _comparison_projection(comparison_summary)
    comparison_baseline = _comparison_baseline()
    if comparison_projection != comparison_baseline:
        raise AssertionError(
            f"P21 comparison baseline mismatch: {comparison_projection!r} != {comparison_baseline!r}",
        )

    real_engine_pass_count = 0
    if real_engine_command:
        real_run_out = work_dir / "real_engine_run"
        real_reports = work_dir / "real_engine_reports"
        _run(
            [
                sys.executable,
                "-m",
                "cdisc_rulekit.cli",
                "run-core",
                "--generated-rules",
                str(generated / "generated_rules"),
                "--out",
                str(real_run_out),
                "--engine-command",
                real_engine_command,
            ],
            env=env,
        )
        _run(
            [
                sys.executable,
                "-m",
                "cdisc_rulekit.cli",
                "compare-results",
                "--generated-rules",
                str(generated / "generated_rules"),
                "--actual-root",
                str(real_run_out / "core_runs"),
                "--out",
                str(real_reports),
                "--strict-structure",
            ],
            env=env,
        )
        real_summary = json.loads(
            (real_reports / "comparison_summary.json").read_text(encoding="utf-8"),
        )
        real_engine_pass_count = int(real_summary["pass_count"])

    _write_failure_probe_actuals(generated / "generated_rules", failure_probe / "core_runs")
    _run_expect_failure(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "compare-results",
            "--generated-rules",
            str(generated / "generated_rules"),
            "--actual-root",
            str(failure_probe / "core_runs"),
            "--out",
            str(failure_probe / "reports"),
            "--strict-structure",
        ],
        env=env,
    )
    failure_probe_summary = json.loads(
        (failure_probe / "reports" / "comparison_summary.json").read_text(encoding="utf-8"),
    )

    fuzzy_probe = _fuzzy_mapping_probe()
    failure_probe_failed_cases = _failure_case_projection(failure_probe_summary)
    failure_probe_directions = _failure_direction_counts(failure_probe_failed_cases)
    summary = {
        "build_readonly_mapping_rows": len(mapping_rows),
        "comparison_fail_count": comparison_summary["fail_count"],
        "comparison_pass_count": comparison_summary["pass_count"],
        "comparison_projection_rows": comparison_projection["rows"],
        "duplicate_probe_unique_keys": _duplicate_rule_id_probe(),
        "failure_probe_extra_issue_fail_count": failure_probe_directions["extra_issue"],
        "failure_probe_equal_count_mismatch_fail_count": failure_probe_directions[
            "equal_count_mismatch"
        ],
        "failure_probe_fail_count": failure_probe_summary["fail_count"],
        "failure_probe_failed_cases": failure_probe_failed_cases,
        "failure_probe_missing_issue_fail_count": failure_probe_directions["missing_issue"],
        "fuzzy_probe_confidence_above_threshold": fuzzy_probe["confidence_above_threshold"],
        "fuzzy_probe_match_type": fuzzy_probe["match_type"],
        "generated_count": generation_summary["generated_count"],
        "generated_skipped_count": generation_summary["skipped_count"],
        "run_core_pass_count": sum(1 for row in run_summary_rows if row["status"] == "PASS"),
        "real_engine_pass_count": real_engine_pass_count,
        "unsupported_probe_generated_count": unsupported_generation_summary["generated_count"],
        "unsupported_probe_skipped_count": unsupported_generation_summary["skipped_count"],
    }
    reports.mkdir(parents=True, exist_ok=True)
    (reports / "p21port_smoke_summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the lightweight P21PORT smoke workflow.")
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument(
        "--real-engine-command",
        help="also execute and strictly compare generated cases with this engine command",
    )
    args = parser.parse_args()
    run(args.work_dir, real_engine_command=args.real_engine_command)
    print("p21port smoke complete: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
