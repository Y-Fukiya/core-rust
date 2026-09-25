"""Exercise the regression baseline availability guard from the actual workflow."""

import os
from pathlib import Path
import subprocess

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
