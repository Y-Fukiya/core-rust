"""Exercise the regression baseline availability guard from the actual workflow."""

import json
import os
from pathlib import Path
import subprocess
import sys

import pytest
import yaml


@pytest.mark.parametrize("present", [False, True], ids=["missing", "present"])
def test_upstream_regression_requires_baseline(tmp_path: Path, present: bool) -> None:
    repo = Path(__file__).resolve().parents[1]
    workflow = yaml.safe_load(
        (repo / ".github/workflows/open-rules-upstream.yml").read_text(encoding="utf-8")
    )
    steps = workflow["jobs"]["upstream-regression"]["steps"]
    guard = next(step for step in steps if step.get("id") == "baseline")
    assert "if" not in guard
    baseline = tmp_path / "tests/open_rules/upstream-baseline.json"
    if present:
        baseline.parent.mkdir(parents=True)
        baseline.write_text("{}", encoding="utf-8")
    output = tmp_path / "github-output"
    result = subprocess.run(
        ["bash", "-e", "-c", guard["run"]],
        cwd=tmp_path,
        env={**os.environ, "GITHUB_OUTPUT": str(output)},
        capture_output=True,
        text=True,
        check=False,
    )
    assert (result.returncode == 0) is present
    if not present:
        assert "::error::" in result.stdout
        assert "tests/open_rules/upstream-baseline.json" in result.stdout


def test_candidate_diagnostic_does_not_replace_accepted_schedule() -> None:
    repo = Path(__file__).resolve().parents[1]
    workflow = yaml.safe_load(
        (repo / ".github/workflows/open-rules-upstream.yml").read_text(encoding="utf-8")
    )
    trigger = workflow.get("on", workflow.get(True))
    profile = trigger["workflow_dispatch"]["inputs"]["profile"]
    assert profile["default"] == "accepted"
    assert profile["options"] == ["accepted", "cdisc-v1-candidate"]
    assert trigger["schedule"]
    output = workflow["jobs"]["candidate-linux"]["steps"][0]["run"]
    assert "$RUNNER_TEMP/" in output
    assert "$GITHUB_RUN_ID" in output
    assert "$GITHUB_RUN_ATTEMPT" in output
    for name in ("upstream-observe", "upstream-regression"):
        assert workflow["jobs"][name]["if"] == (
            "github.event_name != 'workflow_dispatch' || inputs.profile != 'cdisc-v1-candidate'"
        )
    for name in ("candidate-linux", "candidate-official"):
        job = workflow["jobs"][name]
        assert job["if"] == (
            "github.event_name == 'workflow_dispatch' && inputs.profile == 'cdisc-v1-candidate'"
        )
        assert job["runs-on"] == "ubuntu-22.04"
        assert job["permissions"] == {"contents": "read"}
        uploads = [step for step in job["steps"] if step.get("uses", "").startswith("actions/upload-artifact@")]
        assert len(uploads) == 1
        assert uploads[0]["if"] == "always()"
        assert uploads[0]["with"]["if-no-files-found"] == "error"


@pytest.mark.parametrize(
    ("status", "gate_failed", "passes"),
    [(0, False, True), (1, True, True), (137, True, False), (0, True, False), (1, False, False)],
)
def test_candidate_diagnostic_validates_process_exit(tmp_path, status, gate_failed, passes):
    repo = Path(__file__).resolve().parents[1]
    workflow = yaml.safe_load((repo / ".github/workflows/open-rules-upstream.yml").read_text())
    step = next(s for s in workflow["jobs"]["candidate-linux"]["steps"] if s.get("name") == "Run candidate default and strict scoring")
    validation = step["run"].split("python - <<'PY'\n", 1)[1].split("\nPY\n", 1)[0]
    out = tmp_path / "target/candidate-audit"
    (out / "results").mkdir(parents=True)
    (out / "results/run-summary.json").write_text(json.dumps({"failed": 0, "total_cases": 2298, "succeeded": 2298}))
    (out / "process-status.json").write_text(json.dumps({"default_exit": status, "strict_exit": status}))
    (out / "conditions.json").write_text(json.dumps({"protected_sha256": {}}))
    for mode in ("default", "strict"):
        (out / mode).mkdir()
        (out / mode / "scoreboard.json").write_text(json.dumps({
            "summary": {"total_cases": 2298, "harness_error": 0},
            "gate": {"should_fail": gate_failed},
        }))
    result = subprocess.run(
        [sys.executable, "-c", validation], cwd=tmp_path, capture_output=True, text=True,
        env={**os.environ, "CANDIDATE_AUDIT_OUT": str(out)},
    )
    assert (result.returncode == 0) is passes, result.stderr
