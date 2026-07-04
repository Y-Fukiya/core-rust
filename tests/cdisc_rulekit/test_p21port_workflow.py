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
        "build_readonly_mapping_rows": 2,
        "comparison_fail_count": 0,
        "comparison_pass_count": 2,
        "generated_count": 1,
        "run_core_pass_count": 2,
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
