"""A source-text review is not approval, and a fixture check is not an engine run."""

import copy
from collections import Counter
import csv
import importlib.util
import json
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("pilot_check", REPO / "scripts/open_rules_pilot_check.py")
pilot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pilot)


@pytest.fixture
def review(tmp_path):
    rule_dir = tmp_path / "Published/CORE-000001"
    for kind in ("negative", "positive"):
        path = rule_dir / kind / "01/results/results.csv"
        path.parent.mkdir(parents=True)
        path.write_text("Dataset,Record,Variable,Value\n" + ("IE,1,IEORRES,Y\n" if kind == "negative" else ""))
    (rule_dir / "rule.yml").write_text("Description: synthetic unit fixture\n")
    manifest = {"schema_version": 1, "upstream_sha": "a" * 40,
                "review": {"human_approval": "pending"},
                "rules": [{"rule_id": "CORE-000001", "dataset": "IE", "records": [1],
                           "variables": ["IEORRES"], "basis": "Synthetic unit fixture",
                           "row_context": [{"usubjid": "S1", "seq": "1"}],
                           "source_sha256": pilot.source_hash(rule_dir)}]}
    return manifest, tmp_path


def candidate(report):
    return {"cases": [{**case, "scope": "Published", "bucket": "supported_match",
                       "scoring_policy": "strict_identity", "scoring_normalizations": [],
                       "official_issue_count": case["expected_issue_count"],
                       "candidate_issue_count": case["expected_issue_count"]}
                      for case in report["cases"]]}


def test_fixture_check_does_not_claim_candidate_execution_or_approval(review):
    report = pilot.check_sources(*review)
    assert report["fixture_cross_check"] == "passed"
    assert report["candidate_check"] == "not_performed"
    assert report["review"]["human_approval"] == "pending"
    pilot.check_scoreboard(report, candidate(report))
    assert report["candidate_check"] == "not_performed"
    assert report["review"]["human_approval"] == "pending"


@pytest.mark.parametrize("mutation", [None, "subject", "seq", "variable", "row", "duplicate", "skip", "empty"])
def test_candidate_full_identity_is_checked_separately_from_strict_audit(review, mutation):
    manifest, root = review
    for kind in ("negative", "positive"):
        path = root / "candidate/Published/CORE-000001" / kind / "01/report.csv"
        path.parent.mkdir(parents=True)
        header = "rule_id,execution_status,dataset,domain,row,variables,usubjid,seq\n"
        row = "CORE-000001,failed,IE,IE,1,IEORRES,S1,1\n"
        if kind == "positive":
            row = "CORE-000001,passed,IE,IE,,,,\n"
        elif mutation == "subject":
            row = row.replace("S1", "S2")
        elif mutation == "seq":
            row = row.replace("S1,1", "S1,2")
        elif mutation == "variable":
            row = row.replace("IEORRES", "IECAT")
        elif mutation == "row":
            row = row.replace("IE,IE,1", "IE,IE,2")
        elif mutation == "duplicate":
            row += row
        elif mutation == "skip":
            row = row.replace("failed", "skipped")
        elif mutation == "empty":
            row = ""
        path.write_text(header + row)
    report = pilot.check_sources(manifest, root)
    strict = candidate(report)
    strict["cases"][0]["bucket"] = "supported_mismatch"
    pilot.check_scoreboard(report, strict)
    assert report["strict_audit_buckets"] == {"supported_match": 1, "supported_mismatch": 1}
    if mutation:
        with pytest.raises(ValueError):
            pilot.check_candidate_reports(manifest, report, root / "candidate")
        assert report["candidate_check"] == "not_performed"
    else:
        pilot.check_candidate_reports(manifest, report, root / "candidate")
        assert report["candidate_check"] == "passed"


def test_changed_source_is_rejected_before_oracle_comparison(review):
    manifest, root = review
    (root / "Published/CORE-000001/rule.yml").write_text("changed")
    with pytest.raises(ValueError, match="source changed"):
        pilot.check_sources(manifest, root)


def test_expectations_are_not_derived_from_oracle(review):
    manifest, root = review
    manifest["rules"][0]["records"] = [2]
    with pytest.raises(ValueError, match="expectation differs"):
        pilot.check_sources(manifest, root)


def test_duplicate_oracle_findings_are_not_lost(review):
    manifest, root = review
    rule_dir = root / "Published/CORE-000001"
    path = rule_dir / "negative/01/results/results.csv"
    path.write_text(path.read_text() + "IE,1,IEORRES,Y\n")
    manifest["rules"][0]["source_sha256"] = pilot.source_hash(rule_dir)
    with pytest.raises(ValueError, match="expectation differs"):
        pilot.check_sources(manifest, root)


@pytest.mark.parametrize("change", ["missing", "duplicate", "scope", "bucket", "count", "normalization", "extra"])
def test_candidate_must_match_all_reviewed_cases(review, change):
    report = pilot.check_sources(*review)
    scoreboard = candidate(report)
    first = scoreboard["cases"][0]
    if change == "missing":
        scoreboard["cases"].pop()
    elif change == "duplicate":
        scoreboard["cases"].append(copy.deepcopy(first))
    elif change == "scope":
        first["scope"] = "Unpublished"
    elif change == "bucket":
        first["bucket"] = "deferred_oracle_gap_skipped"
    elif change == "count":
        first["candidate_issue_count"] += 1
    elif change == "normalization":
        first["scoring_normalizations"] = ["row_locator_identity_relaxed"]
    else:
        first["extra"] = [{"row": 10}]
    with pytest.raises(ValueError):
        pilot.check_scoreboard(report, scoreboard)
    assert report["candidate_check"] == "not_performed"


def test_manifest_rejects_duplicate_ids(review, tmp_path):
    manifest, _ = review
    manifest["rules"].append(copy.deepcopy(manifest["rules"][0]))
    path = tmp_path / "review.json"
    path.write_text(json.dumps(manifest))
    with pytest.raises(ValueError, match="duplicate"):
        pilot.load_manifest(path)


def test_committed_pilot_is_bounded_and_not_human_approved():
    manifest = pilot.load_manifest(pilot.DEFAULT_MANIFEST)
    assert 10 <= len(manifest["rules"]) <= 20
    assert manifest["review"]["human_approval"] == "pending"
    lock = dict(line.split("=", 1) for line in (REPO / "tests/open_rules/upstream.lock").read_text().splitlines() if "=" in line)
    assert manifest["upstream_sha"] == lock["sha"]


@pytest.fixture(params=[("CORE-000042", "TT"), ("CORE-000043", "TP")])
def domain_presence_review(request, tmp_path):
    rule_id, variable = request.param
    manifest = json.loads(pilot.DEFAULT_MANIFEST.read_text())
    rule = next(rule for rule in manifest["rules"] if rule["rule_id"] == rule_id)
    manifest["rules"] = [rule]
    manifest["candidate_evidence"] = {
        "official-core-v1-domain-presence": {
            "basis": "Official CORE v1.0 reproduced on the accepted inputs.",
            "reference": "docs/open-rules-domain-presence-reporting.md",
        }
    }
    rule["candidate_expectations"] = {
        "evidence": "official-core-v1-domain-presence",
        "negative": [{"rule_id": rule_id, "dataset": "STUDY", "domain": "", "row": "",
                      "variable": variable, "usubjid": "", "seq": ""}],
        "positive": [],
    }
    rule_dir = tmp_path / "Published" / rule_id
    for kind in ("negative", "positive"):
        path = rule_dir / kind / "01/results/results.csv"
        path.parent.mkdir(parents=True)
        path.write_text("Dataset,Record,Variable,Value\n" + (
            f"{variable},1,{variable},{variable.lower()}.csv\n" if kind == "negative" else ""))
    (rule_dir / "rule.yml").write_text("Description: synthetic domain presence fixture\n")
    rule["source_sha256"] = pilot.source_hash(rule_dir)
    path = tmp_path / "review.json"
    path.write_text(json.dumps(manifest))
    return pilot.load_manifest(path), tmp_path


@pytest.mark.parametrize("mutation", [
    None, "physical", "dataset", "domain", "row", "usubjid", "seq", "variable",
    "rule_id", "missing", "duplicate", "positive", "skip", "positive_skip", "empty",
])
def test_domain_presence_candidate_identity_is_exact(domain_presence_review, mutation):
    manifest, root = domain_presence_review
    rule = manifest["rules"][0]
    rule_id, variable = rule["rule_id"], rule["variables"][0]
    fields = ["rule_id", "execution_status", "dataset", "domain", "row", "variables", "usubjid", "seq"]
    finding = dict(zip(fields, [rule_id, "failed", "STUDY", "", "", variable, "", ""]))
    for kind in ("negative", "positive"):
        row = finding.copy()
        if kind == "positive":
            row.update(execution_status="passed", variables="")
            if mutation == "positive":
                row = finding.copy()
            elif mutation == "positive_skip":
                row["execution_status"] = "skipped"
        elif mutation == "physical":
            row.update(dataset=variable, domain=variable, row="1")
        elif mutation in {"dataset", "domain", "row", "usubjid", "seq", "rule_id"}:
            row[mutation] = "wrong"
        elif mutation == "variable":
            row["variables"] = "WRONG"
        elif mutation == "missing":
            row["execution_status"] = "passed"
        elif mutation == "skip":
            row["execution_status"] = "skipped"
        rows = [row]
        if kind == "negative" and mutation == "duplicate":
            rows.append(row)
        elif kind == "negative" and mutation == "empty":
            rows = []
        path = root / "candidate/Published" / rule_id / kind / "01/report.csv"
        path.parent.mkdir(parents=True)
        with path.open("w", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=fields)
            writer.writeheader()
            writer.writerows(rows)
    report = pilot.check_sources(manifest, root)
    strict = candidate(report)
    strict["cases"][0].update(bucket="supported_mismatch", missing_count=1, extra_count=1,
                              missing=[{"dataset": variable, "row": "1"}],
                              extra=[{"dataset": "STUDY", "row": ""}])
    original_strict = copy.deepcopy(strict)
    pilot.check_scoreboard(report, strict)
    assert report["strict_audit_buckets"] == {"supported_match": 1, "supported_mismatch": 1}
    assert strict == original_strict
    if mutation:
        with pytest.raises(ValueError):
            pilot.check_candidate_reports(manifest, report, root / "candidate")
        assert report["candidate_check"] == "not_performed"
    else:
        pilot.check_candidate_reports(manifest, report, root / "candidate")
        assert report["candidate_check"] == "passed"


@pytest.mark.parametrize("mutation", [
    "null", "list", "empty", "extra_key", "missing_evidence", "unknown_evidence",
    "null_evidence", "list_evidence", "empty_evidence", "missing_registry", "null_registry",
    "list_registry", "null_entry", "string_entry", "missing_basis", "empty_basis",
    "blank_basis", "nonstring_basis", "missing_negative", "null_negative", "object_negative",
    "missing_positive", "null_positive", "object_positive", "null_identity", "list_identity",
    "extra_identity_key", "wrong_rule", "wrong_variable", "missing_finding", "duplicate_finding",
    "positive_finding", "variable_multiplicity",
])
def test_manifest_rejects_malformed_candidate_overrides(domain_presence_review, mutation):
    manifest, root = domain_presence_review
    rule = manifest["rules"][0]
    override = rule["candidate_expectations"]
    evidence = manifest["candidate_evidence"][override["evidence"]]
    if mutation in {"null", "list", "empty"}:
        rule["candidate_expectations"] = {"null": None, "list": [], "empty": {}}[mutation]
    elif mutation == "extra_key":
        override["typo"] = []
    elif mutation == "missing_evidence":
        del override["evidence"]
    elif mutation in {"unknown_evidence", "null_evidence", "list_evidence", "empty_evidence"}:
        override["evidence"] = {"unknown_evidence": "unknown", "null_evidence": None,
                                "list_evidence": [], "empty_evidence": ""}[mutation]
    elif mutation == "missing_registry":
        del manifest["candidate_evidence"]
    elif mutation in {"null_registry", "list_registry"}:
        manifest["candidate_evidence"] = None if mutation == "null_registry" else []
    elif mutation in {"null_entry", "string_entry"}:
        manifest["candidate_evidence"][override["evidence"]] = None if mutation == "null_entry" else "basis"
    elif mutation == "missing_basis":
        del evidence["basis"]
    elif mutation in {"empty_basis", "blank_basis", "nonstring_basis"}:
        evidence["basis"] = {"empty_basis": "", "blank_basis": " \n", "nonstring_basis": 1}[mutation]
    elif mutation.startswith(("missing_negative", "missing_positive")):
        del override[mutation.removeprefix("missing_")]
    elif mutation in {"null_negative", "null_positive", "object_negative", "object_positive"}:
        prefix, kind = mutation.split("_")
        override[kind] = None if prefix == "null" else {}
    elif mutation in {"null_identity", "list_identity"}:
        override["negative"][0] = None if mutation == "null_identity" else []
    elif mutation == "extra_identity_key":
        override["negative"][0]["value"] = "tt.csv"
    elif mutation == "wrong_rule":
        override["negative"][0]["rule_id"] = "CORE-000001"
    elif mutation == "wrong_variable":
        override["negative"][0]["variable"] = "WRONG"
    elif mutation == "missing_finding":
        override["negative"] = []
    elif mutation == "duplicate_finding":
        override["negative"] *= 2
    elif mutation == "positive_finding":
        override["positive"] = copy.deepcopy(override["negative"])
    elif mutation == "variable_multiplicity":
        rule["variables"].append("OTHER")
        override["negative"] *= 2
    path = root / "malformed.json"
    path.write_text(json.dumps(manifest))
    with pytest.raises(ValueError, match="candidate"):
        pilot.load_manifest(path)


@pytest.mark.parametrize("field", ["rule_id", "dataset", "domain", "row", "variable", "usubjid", "seq"])
@pytest.mark.parametrize("value", [None, 1, False, [], {}, "missing-key"])
def test_candidate_identity_fields_are_required_strings(domain_presence_review, field, value):
    manifest, root = domain_presence_review
    identity = manifest["rules"][0]["candidate_expectations"]["negative"][0]
    if value == "missing-key":
        del identity[field]
    else:
        identity[field] = value
    path = root / "malformed.json"
    path.write_text(json.dumps(manifest))
    with pytest.raises(ValueError, match="candidate"):
        pilot.load_manifest(path)


def test_candidate_overrides_do_not_relax_source_hashes(domain_presence_review):
    manifest, root = domain_presence_review
    (root / "Published" / manifest["rules"][0]["rule_id"] / "rule.yml").write_text("changed")
    with pytest.raises(ValueError, match="source changed"):
        pilot.check_sources(manifest, root)


def test_legacy_candidate_identities_keep_record_context(review):
    manifest, root = review
    path = root / "review.json"
    path.write_text(json.dumps(manifest))
    rule = pilot.load_manifest(path)["rules"][0]
    assert pilot.candidate_expected_identities(rule, "negative") == Counter({
        ("CORE-000001", "IE", "IE", "1", "IEORRES", "S1", "1"): 1,
    })
    assert pilot.candidate_expected_identities(rule, "positive") == Counter()


def test_committed_domain_presence_overrides_keep_frozen_sources():
    manifest = pilot.load_manifest(pilot.DEFAULT_MANIFEST)
    overrides = {rule["rule_id"]: rule for rule in manifest["rules"] if "candidate_expectations" in rule}
    assert set(overrides) == {"CORE-000042", "CORE-000043"}
    for rule_id, variable, digest in [
        ("CORE-000042", "TT", "bc66ff89886e1dea79e31261cab87e1bc5675327b6001c9042eb5c1551bdecf8"),
        ("CORE-000043", "TP", "e34298d3675411fcbcc67859f1783d2d734a8cfd344e59de590a437d41e11ddc"),
    ]:
        rule = overrides[rule_id]
        assert rule["dataset"] == variable
        assert rule["records"] == [1]
        assert rule["row_context"] == [{"usubjid": "", "seq": ""}]
        assert rule["source_sha256"] == digest
        assert pilot.expected_rows(rule, "negative") == Counter({(variable, "1", variable): 1})
        assert pilot.candidate_expected_identities(rule, "negative") == Counter({
            (rule_id, "STUDY", "", "", variable, "", ""): 1,
        })
        assert pilot.candidate_expected_identities(rule, "positive") == Counter()
        assert rule["candidate_expectations"]["evidence"] == "official-core-v1-domain-presence"
