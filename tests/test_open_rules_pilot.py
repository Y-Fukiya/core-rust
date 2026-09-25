"""A source-text review is not approval, and a fixture check is not an engine run."""

import copy
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
    assert report["candidate_check"] == "passed"
    assert report["review"]["human_approval"] == "pending"


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
