"""Bounded diagnostic tests; synthetic checks do not establish conformance."""

from copy import deepcopy
import csv
import hashlib
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest
import yaml


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/open_rules_candidate_triage.py"
SPEC = importlib.util.spec_from_file_location("candidate_triage", SCRIPT)
triage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(triage)


def case(rule="CORE-000001", bucket="supported_mismatch", case_id="01"):
    return {
        "scope": "Published", "rule_id": rule, "case_kind": "negative", "case_id": case_id,
        "bucket": bucket, "official_issue_count": 1, "candidate_issue_count": 2,
        "missing_count": 0, "extra_count": 1,
        "extra": [{"rule_id": rule, "dataset": "AE", "domain": "AE", "row": "",
                   "variables": ["AESTAT"], "usubjid": "", "seq": ""}],
        "official_results_csv": "/historical/official/results.csv",
        "candidate_report_csv": "/historical/candidate/report.csv",
    }


def board(cases):
    return {
        "upstream": {"repo": "https://github.com/cdisc-org/cdisc-open-rules.git",
                     "observed_sha": "a" * 40, "expected_sha": "b" * 40},
        "summary": {"total_cases": len(cases), **{b: sum(c["bucket"] == b for c in cases) for b in triage.BUCKETS}},
        "cases": cases,
    }


def pin_tree(root, monkeypatch):
    entries = []
    for path in sorted(root.rglob("rule.yml")):
        content = path.read_bytes()
        blob = hashlib.sha1(b"blob " + str(len(content)).encode("ascii") + b"\0" + content).hexdigest()
        entries.append(f"100644 blob {blob}\t{path.relative_to(root).as_posix()}\0".encode())
    tree = b"".join(entries)
    monkeypatch.setattr(triage.subprocess, "run", lambda args, **kwargs: SimpleNamespace(
        stdout=tree if "ls-tree" in args else "a" * 40 + "\n"
    ))


@pytest.fixture
def inputs(tmp_path, monkeypatch):
    root = tmp_path / "rules"
    (root / ".git").mkdir(parents=True)
    rule_path = root / "Published/CORE-000001/rule.yml"
    rule_path.parent.mkdir(parents=True)
    rule_path.write_text(yaml.safe_dump({
        "Core": {"Id": "CORE-000001"}, "Rule Type": "Record Data", "Sensitivity": "Dataset",
        "Check": {"all": [{"name": "AESTAT", "operator": "exists"}]},
        "Authorities": [{"Organization": "CDISC", "Standards": [{"Name": "SDTMIG", "Version": "3.4"}]}],
        "Outcome": {"Output Variables": ["AESTAT"]},
    }))
    default, strict = tmp_path / "default.json", tmp_path / "strict.json"
    default.write_text(json.dumps(board([case()])))
    strict.write_text(default.read_text())
    pin_tree(root, monkeypatch)
    return default, strict, root


def test_complete_report_and_outputs(inputs, tmp_path):
    default, strict, root = inputs
    snapshots = {p: p.read_bytes() for p in (default, strict, root / "Published/CORE-000001/rule.yml")}
    report = triage.build_report(*inputs)
    assert report["rules_git_head"] == "a" * 40
    assert report["upstream"]["expected_sha"] == "b" * 40
    item = report["cases"][0]
    assert item["primary_family"] == "dataset_sensitivity"
    assert item["traits"]["sensitivity"] == "Dataset"
    assert item["traits"]["authorities"][0]["Standards"][0]["Name"] == "SDTMIG"
    assert item["traits"]["output_variables"] == ["AESTAT"]
    assert item["traits"]["operator_paths"] == {"Check.all[0].operator": "exists"}
    assert len(item["rule_yaml_sha256"]) == 64
    assert len(item["rule_git_blob"]) == 40
    assert item["scoreboard_evidence"]["default"] == json.loads(default.read_text())["cases"][0]
    assert item["suspected_causes"][0]["status"] == "suspected"
    assert "not_raw" in report["evidence_kind"]
    out = tmp_path / "report"
    triage.write_report(report, out)
    assert json.loads((out / "triage.json").read_text()) == report
    with (out / "triage.csv").open() as stream:
        rows = list(csv.DictReader(stream))
    assert len(rows) == 1
    assert rows[0]["case_id"] == "01"
    assert rows[0]["default_extra_count"] == "1"
    assert json.loads(rows[0]["default_extra"]) == item["scoreboard_evidence"]["default"]["extra"]
    assert "dataset_sensitivity | 1 | 0 | 1" in (out / "triage.md").read_text()
    with pytest.raises(FileExistsError):
        triage.write_report(report, out)
    assert all(p.read_bytes() == value for p, value in snapshots.items())


@pytest.mark.parametrize("field", ["repo", "observed_sha", "expected_sha"])
def test_paired_identity_guard(inputs, field):
    default, strict, _ = inputs
    data = json.loads(strict.read_text())
    data["upstream"][field] = "c" * 40
    strict.write_text(json.dumps(data))
    with pytest.raises(triage.InputError, match=f"paired upstream {field}"):
        triage.build_report(*inputs)


def test_rules_head_guard(inputs, monkeypatch):
    monkeypatch.setattr(triage.subprocess, "run", lambda *a, **kw: SimpleNamespace(stdout="c" * 40))
    with pytest.raises(triage.InputError, match="git HEAD"):
        triage.build_report(*inputs)


@pytest.mark.parametrize("mutation,match", [
    (lambda b: b["cases"].append(deepcopy(b["cases"][0])), "duplicate case"),
    (lambda b: b["summary"].update(total_cases=2), "declared count"),
    (lambda b: b["summary"].update(supported_mismatch=0), "declared count"),
    (lambda b: b["summary"].update(total_cases=True), "nonnegative integer"),
    (lambda b: b["cases"][0].update(bucket="unknown"), "unknown bucket"),
    (lambda b: b["cases"][0].update(case_id=1), "path-component strings"),
    (lambda b: b["cases"][0].update(scope="../escape"), "path-component strings"),
    (lambda b: b["cases"][0].update(candidate_issue_count=-1), "nonnegative integer"),
    (lambda b: b["cases"][0].update(candidate_issue_count=3), "inconsistent structural counts"),
    (lambda b: b["cases"][0].update(extra_count=0), "sample exceeds"),
    (lambda b: b["cases"][0].update(extra=["bad"]), "structural row"),
    (lambda b: b["cases"][0].pop("missing_count"), "missing missing_count"),
    (lambda b: b["upstream"].update(observed_sha=None), "invalid upstream"),
    (lambda b: b.update(by_scope=[{"name": "Published", "summary": {}}]), "nonnegative integer"),
])
def test_invalid_scoreboards(inputs, mutation, match):
    default, _, _ = inputs
    data = json.loads(default.read_text())
    mutation(data)
    default.write_text(json.dumps(data))
    with pytest.raises(triage.InputError, match=match):
        triage.build_report(*inputs)


@pytest.mark.parametrize("source", ['{"cases":', '[]', '{"cases": [], "cases": []}', '{"cases": NaN}'])
def test_malformed_json(inputs, source):
    inputs[0].write_text(source)
    with pytest.raises(ValueError):
        triage.build_report(*inputs)


@pytest.mark.parametrize("source,match", [
    ("Core: {Id: CORE-000001}\nRule Type: Record Data\nRule Type: JSONata\n", "duplicate key"),
    ("Core: {Id: CORE-000001}\nCheck: {operator: empty, operator: exists}\n", "duplicate key"),
    ("Core: {Id: WRONG}\n", "Core.Id"),
    ("- list\n", "must be a mapping"),
    ("Core: {Id: CORE-000001}\nOperations: wrong\n", "Operations"),
])
def test_invalid_yaml(inputs, source, match, monkeypatch):
    (inputs[2] / "Published/CORE-000001/rule.yml").write_text(source)
    pin_tree(inputs[2], monkeypatch)
    with pytest.raises(triage.InputError, match=match):
        triage.build_report(*inputs)


def test_dirty_yaml_rejected_even_when_head_matches(inputs):
    rule = inputs[2] / "Published/CORE-000001/rule.yml"
    rule.write_text(rule.read_text().replace("Sensitivity: Dataset", "Sensitivity: Record"))
    with pytest.raises(triage.InputError, match="differ from tracked HEAD blob"):
        triage.build_report(*inputs)


def test_untracked_yaml_rejected(inputs, monkeypatch):
    monkeypatch.setattr(triage.subprocess, "run", lambda args, **kwargs: SimpleNamespace(
        stdout=b"" if "ls-tree" in args else "a" * 40 + "\n"
    ))
    with pytest.raises(triage.InputError, match="untracked"):
        triage.build_report(*inputs)


def test_missing_and_escaped_rule(inputs, tmp_path):
    rule = inputs[2] / "Published/CORE-000001/rule.yml"
    content = rule.read_text()
    rule.unlink()
    with pytest.raises(FileNotFoundError):
        triage.build_report(*inputs)
    outside = tmp_path / "outside.yml"
    outside.write_text(content)
    rule.symlink_to(outside)
    with pytest.raises(triage.InputError, match="escapes"):
        triage.build_report(*inputs)


def test_equal_tuple_keys_required(inputs):
    data = json.loads(inputs[1].read_text())
    data["cases"][0]["case_id"] = "1"
    inputs[1].write_text(json.dumps(data))
    with pytest.raises(triage.InputError, match="case key sets differ"):
        triage.build_report(*inputs)


@pytest.mark.parametrize("rule_type,sensitivity,operators,operations,expected", [
    ("Domain Presence Check", "Group", ["exists"], ["distinct"], "domain_presence"),
    ("Variable Metadata Check", "Group", ["exists"], ["distinct"], "variable_metadata"),
    ("Record Data", "Group", ["date_equal_to"], ["distinct"], "group_sensitivity"),
    ("Record Data", "Dataset", ["date_equal_to"], ["distinct"], "dataset_sensitivity"),
    ("Record Data", "Record", ["date_equal_to", "equal_to"], ["distinct"], "record_data_date"),
    ("Record Data", "Record", ["empty", "equal_to"], ["distinct", "map"], "operation_distinct"),
    ("Record Data", "Record", ["empty", "equal_to"], ["map"], "operation_other"),
    ("Record Data", "Record", ["empty", "equal_to"], [], "record_data_comparator"),
    ("Record Data", "Record", ["empty", "not_exists"], [], "record_data_empty_existence"),
    ("Record Data", "Record", ["unknown_operator"], [], "unclassified"),
    ("Unknown Variable Metadata", "Group", ["empty"], ["distinct"], "unclassified"),
    ("Record Data", "Unknown", ["empty"], [], "unclassified"),
])
def test_family_precedence(rule_type, sensitivity, operators, operations, expected):
    result = triage.classify({"Rule Type": rule_type, "Sensitivity": sensitivity,
                             "Check": {"all": [{"operator": op} for op in operators]},
                             "Operations": [{"operator": op} for op in operations]})
    assert result["primary_family"] == expected
    assert result["traits"]["operators"] == sorted(operators)
    assert result["traits"]["operations"] == sorted(operations)


@pytest.mark.parametrize("sensitivity,official,candidate,expected", [
    ("Dataset", 1, 2, True), ("Group", 3, 1, True), ("Record", 1, 2, False),
    ("Dataset", 0, 2, False), ("Dataset", 1, 1, False), ("Dataset", None, 2, False),
])
def test_granularity_is_only_a_supported_suspicion(sensitivity, official, candidate, expected):
    item = case()
    item.update(official_issue_count=official, candidate_issue_count=candidate)
    assert bool(triage._suspicions(item, {"sensitivity": sensitivity})) is expected


def test_ordering_population_separation_and_totals(inputs, tmp_path):
    default, strict, _ = inputs
    mismatch = case()
    deferred = case(bucket="deferred_oracle_gap_mismatch", case_id="02")
    match = case(bucket="supported_match", case_id="03")
    match.update(candidate_issue_count=1, extra_count=0, extra=[])
    skipped = case(bucket="skipped_unsupported", case_id="04")
    absent = case(bucket="no_official_oracle", case_id="05")
    default.write_text(json.dumps(board([absent, deferred, match, mismatch, skipped])))
    strict.write_text(json.dumps(board([case(case_id="03"), skipped, mismatch, deferred, absent])))
    report = triage.build_report(*inputs)
    assert len(report["cases"]) == 5
    assert [c["key"]["case_id"] for c in report["cases"]] == ["01", "02", "03", "04", "05"]
    summary = report["summary"]
    assert summary["default_mismatches"] == 2
    assert summary["strict_only_mismatches"] == 1
    assert sum(c["total"] for c in summary["default_mismatch_families"].values()) == 2
    assert sum(summary["all_case_families"].values()) == 5
    assert summary["default_mismatch_metadata_reconciliation"] == {"Record Data / Dataset": 2}
    assert summary["default_buckets"]["skipped_unsupported"] == 1
    assert summary["default_buckets"]["no_official_oracle"] == 1
    for path in (default, strict):
        data = json.loads(path.read_text())
        data["cases"].reverse()
        path.write_text(json.dumps(data))
    reordered = triage.build_report(*inputs)
    assert reordered == report
    triage.write_report(report, tmp_path / "first")
    triage.write_report(reordered, tmp_path / "second")
    with (tmp_path / "first/mismatches.csv").open() as stream:
        reader = csv.DictReader(stream)
        compact = list(reader)
        assert reader.fieldnames == [
            *triage.KEY_FIELDS, "primary_family", "rule_type", "sensitivity", "operators", "operations",
            "default_bucket", "default_official_issue_count", "default_candidate_issue_count",
            "default_missing_count", "default_extra_count", "rule_yaml_sha256",
        ]
    assert [row["case_id"] for row in compact] == ["01", "02"]
    assert {row["default_bucket"] for row in compact} == triage.MISMATCHES
    assert compact[0]["default_extra_count"] == "1"
    assert compact[0]["operators"] == '["exists"]'
    assert compact[0]["sensitivity"] == "Dataset"
    for name in ("triage.json", "triage.csv", "triage.md", "mismatches.csv"):
        assert (tmp_path / "first" / name).read_bytes() == (tmp_path / "second" / name).read_bytes()


def test_cli_failure_writes_no_output(inputs, tmp_path, capsys):
    inputs[0].write_text("malformed")
    out = tmp_path / "out"
    assert triage.main(["--default-scoreboard", str(inputs[0]), "--strict-scoreboard", str(inputs[1]),
                        "--rules-root", str(inputs[2]), "--out", str(out)]) == 1
    assert "triage:" in capsys.readouterr().err
    assert not out.exists()
