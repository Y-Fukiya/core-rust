import json
import shlex
import sys

import pytest

from cdisc_rulekit.core_runner import build_core_run_plan
from cdisc_rulekit.errors import CliUsageError
from cdisc_rulekit.run_evidence import capture_run_snapshot, execute_recorded_core_run


def _plan(tmp_path, script=""):
    rule = tmp_path / "rules" / "PILOT-001"
    data = rule / "positive" / "01" / "data"
    data.mkdir(parents=True)
    (rule / "rule.yml").write_text("Core:\n  Id: PILOT-001\n")
    (data / "dm.csv").write_bytes(b"abc")
    (data / ".env").write_text("PRODUCT=SDTMIG\nVERSION=3-4\n")
    engine = tmp_path / "engine.py"
    engine.write_text(
        "from pathlib import Path\nimport sys\n"
        "output = Path(sys.argv[sys.argv.index('--output') + 1])\n"
        "output.mkdir(parents=True, exist_ok=True)\n"
        "(output / 'report.json').write_text('{}')\n" + script
    )
    return build_core_run_plan(
        tmp_path / "rules", tmp_path / "outputs",
        engine_command=shlex.join([sys.executable, str(engine)]), dry_run=False,
    )


def test_recorded_run_links_inputs_command_and_outputs_without_claiming_approval(tmp_path):
    plan = _plan(tmp_path)
    record_path = tmp_path / "evidence.json"
    run = execute_recorded_core_run(plan, record_path, workers=2, timeout_seconds=5)

    assert run.ok
    record = json.loads(record_path.read_text())
    assert record["schema_version"] == 1
    assert record["state"] == "completed"
    assert record["approval"] == {"status": "not_reviewed"}
    assert record["comparison_status"] == "not_performed"
    assert record["integrity_ok"] is True
    assert record["settings"]["workers"] == 2
    assert record["settings"]["timeout_seconds"] == 5
    assert record["plan"]["items"][0]["command"] == plan.items[0].command
    source = str((tmp_path / "rules/PILOT-001/positive/01/data/dm.csv").resolve())
    assert record["before"]["inputs"][source]["sha256"] == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    assert any(path.endswith("/.env") for path in record["before"]["inputs"])
    assert record["before"]["executables"][sys.executable]["sha256"]
    report = str((tmp_path / "outputs/PILOT-001/positive/01/report.json").resolve())
    assert record["outputs"][report]["sha256"] == "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    assert record["execution"]["rows"][0]["status"] == "PASS"
    assert record["started_at"] <= record["finished_at"]


def test_recorded_run_refuses_stale_output_even_with_a_new_evidence_filename(tmp_path):
    plan = _plan(tmp_path)
    first = tmp_path / "first.json"
    assert execute_recorded_core_run(plan, first).ok
    before = first.read_bytes()

    with pytest.raises(CliUsageError, match="new output director"):
        execute_recorded_core_run(plan, tmp_path / "second.json")
    assert first.read_bytes() == before
    assert not (tmp_path / "second.json").exists()


def test_zero_exit_without_report_is_not_a_completed_audited_run(tmp_path):
    plan = _plan(tmp_path, "(output / 'report.json').unlink()\n")
    run = execute_recorded_core_run(plan, tmp_path / "evidence.json")

    assert run.execution.ok
    assert not run.ok
    record = json.loads((tmp_path / "evidence.json").read_text())
    assert record["state"] == "failed"
    assert "no report" in record["snapshot_errors"][0]


@pytest.mark.parametrize("change", [
    "data.write_text('changed')",
    "data.unlink()",
    "data.with_name('added.csv').write_text('added')",
    "data.with_name('.env').write_text('VERSION=9-9')",
])
def test_input_changes_cannot_be_reported_as_a_successful_recorded_run(tmp_path, change):
    plan = _plan(tmp_path, "data = Path(sys.argv[sys.argv.index('--dataset-path') + 1])\n" + change + "\n")
    path = tmp_path / "evidence.json"
    run = execute_recorded_core_run(plan, path)
    assert run.execution.ok
    assert not run.integrity_ok
    assert not run.ok
    record = json.loads(path.read_text())
    assert record["state"] == "failed"
    assert record["before"] != record["after"]


@pytest.mark.parametrize("script, timeout, returncode", [
    ("sys.exit(7)\n", 5, 7),
    ("import time\ntime.sleep(2)\n", 0.1, "TIMEOUT"),
])
def test_engine_failure_and_timeout_are_retained_in_evidence(tmp_path, script, timeout, returncode):
    plan = _plan(tmp_path, script)
    path = tmp_path / "evidence.json"
    run = execute_recorded_core_run(plan, path, timeout_seconds=timeout)
    assert not run.ok
    record = json.loads(path.read_text())
    assert record["state"] == "failed"
    assert record["execution"]["rows"][0]["returncode"] == returncode
    assert record["approval"]["status"] == "not_reviewed"


def test_missing_executable_is_a_recorded_failure_not_unknown_success(tmp_path):
    plan = _plan(tmp_path)
    plan.items[0].command[0] = "missing-pilot-engine-0987654321"
    path = tmp_path / "evidence.json"
    assert not execute_recorded_core_run(plan, path).ok
    record = json.loads(path.read_text())
    assert record["before"]["executables"][plan.items[0].command[0]]["sha256"] is None
    assert record["execution"]["rows"][0]["returncode"] == "HARNESS_ERROR"


def test_preexisting_evidence_is_preserved_and_engine_is_not_started(tmp_path):
    plan = _plan(tmp_path)
    path = tmp_path / "evidence.json"
    path.write_text("keep existing evidence")
    with pytest.raises(CliUsageError, match="cannot create new run evidence"):
        execute_recorded_core_run(plan, path)
    assert path.read_text() == "keep existing evidence"
    assert not list((tmp_path / "outputs").rglob("report.json"))


def test_snapshot_reports_permission_errors_without_starting_engine(tmp_path, monkeypatch):
    from pathlib import Path

    plan = _plan(tmp_path)
    original_open = Path.open

    def denied(path, *args, **kwargs):
        if path.name == "dm.csv":
            raise PermissionError("test read denied")
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr(Path, "open", denied)
    with pytest.raises(CliUsageError, match="cannot capture run inputs"):
        capture_run_snapshot(plan, tmp_path)
    assert not (tmp_path / "outputs").exists()


def test_interrupted_run_keeps_started_record_and_does_not_hide_internal_error(tmp_path, monkeypatch):
    import cdisc_rulekit.run_evidence as evidence

    plan = _plan(tmp_path)
    path = tmp_path / "evidence.json"

    def crash(*args, **kwargs):
        raise RuntimeError("unexpected internal error")

    monkeypatch.setattr(evidence, "execute_core_run_plan", crash)
    with pytest.raises(RuntimeError, match="unexpected internal error"):
        execute_recorded_core_run(plan, path)
    record = json.loads(path.read_text())
    assert record["state"] == "started"
    assert record["finished_at"] is None
    assert record["integrity_ok"] is None


def test_snapshot_includes_review_manifest_and_expected_results(tmp_path):
    plan = _plan(tmp_path)
    rule = tmp_path / "rules/PILOT-001"
    (rule / "manifest.json").write_text('{}')
    (rule / "expected_results.csv").write_text('case_type,expected_issue_count\npositive,0\n')
    snapshot = capture_run_snapshot(plan, tmp_path)
    assert str(rule / "manifest.json") in snapshot["inputs"]
    assert str(rule / "expected_results.csv") in snapshot["inputs"]


def test_unreadable_data_directory_is_not_silently_omitted_from_snapshot(tmp_path, monkeypatch):
    import os

    plan = _plan(tmp_path)
    original_scandir = os.scandir

    def denied(path):
        if str(path).endswith("/data"):
            raise PermissionError("test directory listing denied")
        return original_scandir(path)

    monkeypatch.setattr(os, "scandir", denied)
    with pytest.raises(CliUsageError, match="cannot capture run inputs"):
        capture_run_snapshot(plan, tmp_path)
