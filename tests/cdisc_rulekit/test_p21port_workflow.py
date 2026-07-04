import json
import os
import subprocess
import sys

import pytest

import scripts.p21port_smoke as p21port_smoke


@pytest.mark.integration
def test_p21port_smoke_workflow_runs_build_generate_execute_and_compare(tmp_path):
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    result = subprocess.run(
        [
            sys.executable,
            "scripts/p21port_smoke.py",
            "--work-dir",
            str(tmp_path / "p21port"),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
        timeout=180,
    )

    assert "p21port smoke complete: ok" in result.stdout
    summary_path = tmp_path / "p21port" / "reports" / "p21port_smoke_summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    assert summary == {
        "build_readonly_mapping_rows": 3,
        "comparison_fail_count": 0,
        "comparison_pass_count": 4,
        "duplicate_probe_unique_keys": 2,
        "failure_probe_extra_issue_fail_count": 1,
        "failure_probe_fail_count": 2,
        "failure_probe_failed_cases": [
            {
                "actual_issue_count": 0,
                "case_id": "01",
                "case_type": "negative",
                "expected_issue_count": 1,
                "generated_rule_id": "P21PORT-SDTMIG-SD0002-35DD9145",
                "notes": "issue count mismatch",
                "status": "FAIL",
                "variables": "AEDTC|DOMAIN",
            },
            {
                "actual_issue_count": 2,
                "case_id": "01",
                "case_type": "negative",
                "expected_issue_count": 1,
                "generated_rule_id": "P21PORT-SDTMIG-SD0003-1803FB20",
                "notes": "issue count mismatch",
                "status": "FAIL",
                "variables": "AESTDTC|DOMAIN",
            },
        ],
        "failure_probe_missing_issue_fail_count": 1,
        "fuzzy_probe_confidence_above_threshold": True,
        "fuzzy_probe_match_type": "FUZZY",
        "generated_count": 2,
        "generated_skipped_count": 1,
        "run_core_pass_count": 4,
        "unsupported_probe_generated_count": 0,
        "unsupported_probe_skipped_count": 3,
    }


def test_p21port_smoke_subprocess_steps_have_timeout(monkeypatch):
    captured = {}

    def fake_run(args, **kwargs):
        captured["args"] = args
        captured.update(kwargs)

        class Completed:
            stdout = ""
            stderr = ""
            returncode = 0

        return Completed()

    monkeypatch.setattr(p21port_smoke.subprocess, "run", fake_run)

    p21port_smoke._run(["python", "--version"], env={"PYTHONPATH": "src"})

    assert captured["timeout"] == p21port_smoke.SMOKE_STEP_TIMEOUT_SECONDS
