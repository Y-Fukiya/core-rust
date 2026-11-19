"""Exercise the same curated preparation CLI that CI runs."""

import hashlib
import json
from pathlib import Path
import runpy
import shutil
import subprocess
import sys

import pytest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
RULE_206 = "CORE-000206"
RULE_272 = "CORE-000272"
RULE_677 = "CORE-000677"
CASES = ("negative/01", "negative/02", "positive/01", "positive/02")
SHA = "a" * 40
HEADER = b"Dataset,Record,Variable\n"
HISTORICAL = HEADER + b"SUPPDM,1,IDVAR\n" * 12
REVIEWED = {
    RULE_206: {"negative/02": HEADER + b"DM,2,USUBJID\n"},
    RULE_272: {
        "negative/03": b"Dataset,Record,Variable,Value\n",
        "negative/04": b"Dataset,Record,Variable,Value\n",
    },
    RULE_677: {
        "negative/01": HEADER,
        "negative/02": HEADER,
        "positive/02": HEADER + b"SUPPDM,3,IDVAR\nSUPPDM,4,IDVAR\n",
    },
}
source_hash = runpy.run_path(str(SCRIPTS / "open_rules_pilot_check.py"))["source_hash"]


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def original(rule_dir, case):
    return rule_dir / case / "results/results.csv"


def snapshot(root):
    return {path.relative_to(root): path.read_bytes() for path in root.rglob("*") if path.is_file()}


def write_reviewed(fixtures, rule_dir, replacements):
    reviewed = fixtures / "curated-reviewed" / rule_dir.name
    for case, data in replacements.items():
        write(reviewed / case / "results.csv", data)
    manifest = {
        "schema_version": 1,
        "rule_id": rule_dir.name,
        "upstream_sha": SHA,
        "source_sha256": source_hash(rule_dir),
        "replacements": {case: hashlib.sha256(data).hexdigest() for case, data in replacements.items()},
    }
    (reviewed / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")


@pytest.fixture
def corpus(tmp_path):
    scripts = tmp_path / "scripts"
    scripts.mkdir()
    for name in ("prepare_open_rules_curated.py", "open_rules_pilot_check.py"):
        shutil.copyfile(SCRIPTS / name, scripts / name)
    fixtures = tmp_path / "tests/open_rules"
    upstream = tmp_path / "upstream"
    write(fixtures / "curated-upstream-rules.txt", f"  # subset\n\n{RULE_206}\n{RULE_272}\n{RULE_677}\n".encode())
    write(fixtures / "upstream.lock", f"repo=example\nsha={SHA}\n".encode())
    for rule_id, replacements in REVIEWED.items():
        rule_dir = upstream / "Published" / rule_id
        write(rule_dir / "rule.yml", b"core_id: synthetic\n")
        for case in dict.fromkeys((*CASES, *replacements)):
            write(original(rule_dir, case), HISTORICAL if case.startswith("negative") else HEADER)
            write(rule_dir / case / "data/.env", b"STUDY=synthetic\n")
            write(rule_dir / case / "data/suppdm.csv", b"IDVAR\nUSUBJID\n")
        write_reviewed(fixtures, rule_dir, replacements)
    write(upstream / "Published/CORE-000002/rule.yml", b"not selected\n")
    return scripts / "prepare_open_rules_curated.py", fixtures, upstream, tmp_path / "subset"


def prepare(script, upstream, subset):
    return subprocess.run(
        [sys.executable, "-B", str(script), "--upstream-root", str(upstream), "--subset-root", str(subset)],
        capture_output=True, text=True, check=False,
    )


def test_copies_all_278_cases_with_only_reviewed_csvs_replaced(corpus):
    script, fixtures, upstream, subset = corpus
    rule_list = fixtures / "curated-upstream-rules.txt"
    with rule_list.open("a", encoding="utf-8") as stream:
        for number in range(3, 47):
            rule_id = f"CORE-{number:06d}"
            stream.write(f"{rule_id}\n")
            for kind in ("negative", "positive"):
                for case_id in range(1, 4):
                    write(original(upstream / "Published" / rule_id, f"{kind}/{case_id:02d}"), HEADER)

    sources = snapshot(upstream)
    reviews = snapshot(fixtures)
    expected = {path: data for path, data in sources.items() if path.parts[1] != "CORE-000002"}
    for rule_id, replacements in REVIEWED.items():
        for case, data in replacements.items():
            target = original(Path("Published") / rule_id, case)
            assert expected[target] != data
            expected[target] = data
    result = prepare(script, upstream, subset)
    assert result.returncode == 0, result.stderr
    assert len(list(subset.glob("Published/*/*/*/results/results.csv"))) == 278
    assert snapshot(subset) == expected
    assert snapshot(upstream) == sources
    assert snapshot(fixtures) == reviews


@pytest.mark.parametrize(("mutation", "message"), [
    ("missing", "source changed"), ("changed", "source changed"), ("extra", "source changed"),
    ("missing_target", "missing original oracle CSV"), ("replacement", "replacement hash mismatch"),
    ("case_key", "case keys must be exactly"), ("missing_case", "case keys must be exactly"),
    ("pin", "does not match upstream.lock"), ("unknown_rule", "not in the curated subset"),
    ("invalid_rule", "invalid or duplicate curated rule ID"), ("duplicate", "invalid or duplicate curated rule ID"),
    ("source_link", "symlink in upstream"), ("replacement_link", "symlink in reviewed"),
])
def test_rejects_bad_inputs_before_creating_subset(corpus, mutation, message):
    script, fixtures, upstream, subset = corpus
    rule = upstream / "Published" / RULE_677
    manifest_path = fixtures / "curated-reviewed" / RULE_677 / "manifest.json"
    manifest = json.loads(manifest_path.read_bytes())
    replacement = manifest_path.parent / "positive/02/results.csv"
    rules = fixtures / "curated-upstream-rules.txt"
    if mutation == "missing":
        (rule / "rule.yml").unlink()
    elif mutation == "changed":
        (rule / "negative/01/data/.env").write_bytes(b"changed")
    elif mutation == "extra":
        write(rule / "negative/01/data/extra.csv", b"extra")
    elif mutation == "missing_target":
        original(rule, "positive/02").unlink()
        manifest["source_sha256"] = source_hash(rule)
    elif mutation == "replacement":
        replacement.write_bytes(b"changed")
    elif mutation == "case_key":
        manifest["replacements"]["negative/../01"] = manifest["replacements"].pop("negative/01")
    elif mutation == "missing_case":
        del manifest["replacements"]["negative/01"]
    elif mutation == "pin":
        (fixtures / "upstream.lock").write_text(f"sha={'b' * 40}\n", encoding="utf-8")
    elif mutation in {"unknown_rule", "invalid_rule", "duplicate"}:
        tail = {"unknown_rule": "", "invalid_rule": f"../{RULE_677}\n", "duplicate": f"{RULE_677}\n{RULE_677}\n"}
        rules.write_text(f"{RULE_206}\n{tail[mutation]}", encoding="utf-8")
    elif mutation in {"source_link", "replacement_link"}:
        path = original(rule, "negative/01") if mutation == "source_link" else replacement
        link_target = upstream.parent / "same-bytes.csv"
        path.rename(link_target)
        path.symlink_to(link_target)
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

    sources = snapshot(upstream)
    result = prepare(script, upstream, subset)
    assert result.returncode != 0 and message in result.stderr
    assert not subset.exists()
    assert snapshot(upstream) == sources


@pytest.mark.parametrize("rule_id", [RULE_206, RULE_272])
def test_rejects_extra_reviewed_case_before_creating_subset(corpus, rule_id):
    script, fixtures, upstream, subset = corpus
    rule = upstream / "Published" / rule_id
    write_reviewed(fixtures, rule, {**REVIEWED[rule_id], "negative/01": HEADER})

    result = prepare(script, upstream, subset)
    assert result.returncode != 0
    assert "case keys must be exactly" in result.stderr
    assert not subset.exists()


@pytest.mark.parametrize("destination", ["existing", "dangling", "inside_upstream", "aliased_upstream"])
def test_rejects_unsafe_destinations(corpus, destination):
    script, _, upstream, subset = corpus
    if destination == "existing":
        write(subset / "keep.txt", b"keep")
    elif destination == "dangling":
        subset.symlink_to(upstream.parent / "absent")
    elif destination == "inside_upstream":
        subset = upstream / "subset"
    else:
        alias = upstream.parent / "alias"
        alias.symlink_to(upstream, target_is_directory=True)
        subset = alias / "subset"

    result = prepare(script, upstream, subset)
    assert result.returncode != 0
    if destination == "existing":
        assert snapshot(subset) == {Path("keep.txt"): b"keep"}
    elif destination == "dangling":
        assert subset.is_symlink() and not subset.exists()
    else:
        assert not subset.exists()
