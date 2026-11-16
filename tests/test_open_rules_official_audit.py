"""Synthetic runner tests do not establish official CORE execution or conformance."""

import copy
import importlib.util
import json
from pathlib import Path
import subprocess

import pytest


REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("official_audit", REPO / "scripts/open_rules_official_audit.py")
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)
PROFILE = REPO / "tests/open_rules/diagnostic/cdisc-v1-candidate.json"


def report(status="SUCCESS", rule_id="CORE-000013"):
    return {"Rules_Report": [{"core_id": rule_id, "status": status}],
            "Conformance_Details": {"Standard": "SDTMIG"},
            "Issue_Details": [{"Variable": "AESTAT"}] if status == "ISSUE REPORTED" else []}


@pytest.mark.parametrize("status", sorted(audit.STATUSES))
def test_exact_status_schema(tmp_path, status):
    path = tmp_path / "report.json"
    audit.write_json(path, report(status))
    result = audit.inspect_report(path, "CORE-000013")
    assert result["status"] == status
    assert result["issue_details_count"] == (1 if status == "ISSUE REPORTED" else 0)


@pytest.mark.parametrize("mutation", ["wrong-rule", "zero", "missing", "duplicate", "lowercase", "contradiction", "unknown"])
def test_invalid_reports_cannot_be_zero_finding_success(tmp_path, mutation):
    document = report()
    if mutation == "wrong-rule":
        document["Rules_Report"][0]["core_id"] = "CORE-000001"
    elif mutation == "zero":
        document["Rules_Report"] = []
    elif mutation == "missing":
        del document["Rules_Report"]
    elif mutation == "duplicate":
        document["Rules_Report"] *= 2
    elif mutation == "lowercase":
        document["Rules_Report"][0]["status"] = "success"
    elif mutation == "contradiction":
        document["Issue_Details"] = [{}]
    else:
        document["Rules_Report"][0]["status"] = "UNKNOWN STATUS"
    path = tmp_path / "report.json"
    audit.write_json(path, document)
    with pytest.raises(ValueError):
        audit.inspect_report(path, "CORE-000013")


def test_missing_output_is_not_a_pass(tmp_path):
    with pytest.raises(FileNotFoundError):
        audit.inspect_report(tmp_path / "report.json", "CORE-000013")


def test_cache_pin_detects_changed_added_and_missing_files(tmp_path):
    cache = tmp_path / "cache"
    cache.mkdir()
    (cache / "metadata.pkl").write_bytes(b"synthetic cache; never unpickle")
    initial = audit.cache_manifest(cache)
    pins = {"cache_tree_sha256": initial["tree_sha256"], "cache_file_count": 1,
            "cache_files": initial["files"]}
    audit.check_cache(initial, pins)
    (cache / "extra.pkl").write_bytes(b"unexpected")
    with pytest.raises(ValueError, match="cache tree pin"):
        audit.check_cache(audit.cache_manifest(cache), pins)
    (cache / "extra.pkl").unlink()
    for changed in (b"changed", b""):
        (cache / "metadata.pkl").write_bytes(changed)
        with pytest.raises(ValueError, match="cache tree pin"):
            audit.check_cache(audit.cache_manifest(cache), pins)
    (cache / "metadata.pkl").unlink()
    with pytest.raises(ValueError, match="cache tree pin"):
        audit.check_cache(audit.cache_manifest(cache), pins)


def test_environment_does_not_inherit_settings_or_credentials(tmp_path, monkeypatch):
    for key in ("PRODUCT", "VERSION", "CT", "DEFINE_XML", "CDISC_LIBRARY_API_KEY", "AWS_SECRET_ACCESS_KEY", "PYTHONPATH", "LD_PRELOAD"):
        monkeypatch.setenv(key, "must-not-leak")
    env = audit.isolated_environment(tmp_path)
    assert "must-not-leak" not in env.values()
    assert set(env) == {"PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "CACHE_TYPE"}
    assert env["CACHE_TYPE"] == "in_memory"


def test_fixture_environment_rejects_credentials(tmp_path):
    path = tmp_path / ".env"
    path.write_text("PRODUCT=TIG\nVERSION=1-0\nCDISC_LIBRARY_API_KEY=not-a-real-key\n")
    with pytest.raises(ValueError, match="supported settings"):
        audit.read_fixture_environment(path)


def test_existing_output_is_never_overwritten(tmp_path):
    out = tmp_path / "existing"
    out.mkdir()
    sentinel = out / "summary.json"
    sentinel.write_text("keep this")
    with pytest.raises(ValueError, match="already exists"):
        audit.run_audit(PROFILE, tmp_path, tmp_path, out)
    assert sentinel.read_text() == "keep this"


@pytest.fixture
def checkout(tmp_path):
    root = tmp_path / "rules"
    root.mkdir()
    def git(*args):
        return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True).stdout.decode().strip()
    git("init", "-q")
    (root / "README.md").write_text("synthetic rules checkout")
    data = root / "Published/CORE-000013/positive/01/data"
    data.mkdir(parents=True)
    (data / ".env").write_text("PRODUCT=SDTMIG\nVERSION=3-4\n")
    (data / "ae.csv").write_text("DOMAIN\nAE\n")
    (root / "Published/CORE-000013/rule.yml").write_text("synthetic rule, not executed")
    git("add", ".")
    git("update-index", "--add", "--cacheinfo", "160000", "b" * 40, "engine")
    git("-c", "user.name=Audit Test", "-c", "user.email=audit@example.invalid", "commit", "-qm", "synthetic fixture")
    return root, git("rev-parse", "HEAD")


def test_clean_tracked_check_ignores_untracked_but_detects_edits(checkout):
    root, head = checkout
    pins = {"sha": head, "engine_submodule_sha": "b" * 40}
    (root / "unrelated-untracked.txt").write_text("not input")
    assert audit.check_rules(root, pins)["tracked_clean"]
    (root / "README.md").write_text("changed tracked file")
    with pytest.raises(ValueError, match="git diff"):
        audit.check_rules(root, pins)


def test_snapshot_excludes_untracked_input_and_records_hashes(checkout, tmp_path):
    root, _ = checkout
    (root / "Published/CORE-000013/positive/01/data/untracked.csv").write_text("must not be consumed")
    case = audit.load_profile(PROFILE)["cases"][0]
    snapshot = audit.prepare_case(root, case, tmp_path / "snapshot")
    assert len(snapshot["files"]) == 3
    assert not (Path(snapshot["data"]) / "untracked.csv").exists()
    assert all(len(item["sha256"]) == 64 for item in snapshot["files"].values())


def test_all_cases_attempted_and_expected_probe_statuses_preserved(checkout, tmp_path, monkeypatch):
    root, head = checkout
    engine = tmp_path / "engine"
    cache = engine / "resources/cache"
    cache.mkdir(parents=True)
    (cache / "test.pkl").write_bytes(b"synthetic")
    (engine / "core").write_text("not an engine")
    manifest = audit.cache_manifest(cache)
    profile = audit.load_profile(PROFILE)
    profile["rules"].update(sha=head, engine_submodule_sha="b" * 40)
    profile["engine"].update(cache_tree_sha256=manifest["tree_sha256"], cache_file_count=1, cache_files=manifest["files"])
    first = profile["cases"][0]
    statuses = ["SUCCESS", "SKIPPED", "EXECUTION ERROR", "wrong-rule", "no-output", "ISSUE REPORTED"]
    profile["cases"] = [{**copy.deepcopy(first), "label": f"case-{i}",
                         **({"expected_status": status} if status in {"SKIPPED", "EXECUTION ERROR"} else {})}
                        for i, status in enumerate(statuses)]
    profile_path = tmp_path / "profile.json"
    audit.write_json(profile_path, profile)
    monkeypatch.setattr(audit, "check_platform", lambda *args: None)
    calls = []
    def fake_execute(argv, cwd, env, prefix, timeout):
        stdout, stderr = prefix.with_suffix(".stdout.txt"), prefix.with_suffix(".stderr.txt")
        stdout.write_text("1.0.0\n" if argv[-1] == "version" else "synthetic run\n")
        stderr.write_text("")
        result = {"argv": argv, "returncode": 0, "stdout": str(stdout), "stderr": str(stderr)}
        if argv[-1] == "version":
            return result
        calls.append(argv)
        label = prefix.parent.name
        status = statuses[int(label.split("-")[1])]
        if status != "no-output":
            document = report("SUCCESS", "CORE-000001") if status == "wrong-rule" else report(status)
            audit.write_json(prefix.parent / "report.json", document)
            (prefix.parent / "report.csv").write_text("Dataset,Record,Variable,Value\n")
        return result
    monkeypatch.setattr(audit, "execute", fake_execute)
    out = tmp_path / "output"
    assert audit.run_audit(profile_path, root, engine, out) == 1
    summary = json.loads((out / "summary.json").read_text())
    assert len(calls) == len(statuses) == len(summary["cases"])
    assert summary["conformance_claim"] is False
    assert summary["cases"][1]["outcome"] == "skipped"
    assert summary["cases"][2]["outcome"] == "execution_error"
    assert not summary["cases"][1]["errors"] and not summary["cases"][2]["errors"]
    assert summary["cases"][3]["outcome"] == "invalid_report"
    assert summary["cases"][4]["outcome"] == "invalid_report"
    assert summary["cases"][5]["status"] == "ISSUE REPORTED"


def test_preflight_failure_still_writes_summary(tmp_path, monkeypatch):
    monkeypatch.setattr(audit.platform, "system", lambda: "Darwin")
    out = tmp_path / "output"
    assert audit.run_audit(PROFILE, tmp_path, tmp_path, out) == 2
    summary = json.loads((out / "summary.json").read_text())
    assert not summary["audit_complete"]
    assert summary["cases"] == []
    assert "Linux x86-64" in summary["errors"][0]


def test_committed_profile_pins_candidate_without_changing_accepted_lock():
    profile = audit.load_profile(PROFILE)
    assert len(profile["cases"]) == 22
    assert sum("expected_status" in case for case in profile["cases"]) == 2
    assert profile["rules"]["sha"] == "1881cb9a1f9a38b6cafc785eca5169b240981f66"
    assert profile["engine"]["asset_sha256"] == "52a932ca3f6cbc08055da2611c2a37badde78ee1f426999d39d4cbb24467d870"
    assert profile["rules"]["sha"] not in (REPO / "tests/open_rules/upstream.lock").read_text()
