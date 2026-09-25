"""Check frozen pilot expectations without deriving them from engine output."""

import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import re
import shutil


DEFAULT_MANIFEST = Path(__file__).resolve().parents[1] / "tests/open_rules/pilot-review.json"


def source_hash(rule_dir: Path) -> str:
    entries = [
        (path.relative_to(rule_dir).as_posix(), hashlib.sha256(path.read_bytes()).hexdigest())
        for path in sorted(rule_dir.rglob("*"))
        if path.is_file()
    ]
    return hashlib.sha256(json.dumps(entries, separators=(",", ":")).encode()).hexdigest()


def expected_rows(rule: dict, kind: str) -> Counter:
    return Counter(
        (rule["dataset"], str(record), variable)
        for record in (rule["records"] if kind == "negative" else [])
        for variable in rule["variables"]
    )


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1 or not manifest.get("rules"):
        raise ValueError("pilot manifest must have schema_version=1 and nonempty rules")
    ids = set()
    for rule in manifest["rules"]:
        rule_id = rule["rule_id"]
        if not re.fullmatch(r"CORE-\d{6}", rule_id) or rule_id in ids:
            raise ValueError(f"invalid or duplicate pilot rule: {rule_id}")
        ids.add(rule_id)
        if not rule.get("basis") or not re.fullmatch(r"[0-9a-f]{64}", rule.get("source_sha256", "")):
            raise ValueError(f"missing review basis/source hash: {rule_id}")
        records, variables = rule["records"], rule["variables"]
        if (not records or any(type(row) is not int or row < 1 for row in records)
                or len(set(records)) != len(records)
                or not variables or len(set(variables)) != len(variables)):
            raise ValueError(f"invalid expected records/variables: {rule_id}")
    return manifest


def check_sources(manifest: dict, upstream_root: Path) -> dict:
    cases = []
    for rule in manifest["rules"]:
        rule_dir = upstream_root / "Published" / rule["rule_id"]
        if source_hash(rule_dir) != rule["source_sha256"]:
            raise ValueError(f"source changed or missing: {rule['rule_id']}; review before updating expectations")
        for kind in ("negative", "positive"):
            path = rule_dir / kind / "01/results/results.csv"
            with path.open(encoding="utf-8-sig", newline="") as stream:
                reader = csv.DictReader(stream)
                if not {"Dataset", "Record", "Variable"}.issubset(reader.fieldnames or []):
                    raise ValueError(f"missing structural oracle columns: {path}")
                actual = Counter((row["Dataset"], row["Record"], row["Variable"]) for row in reader)
            expected = expected_rows(rule, kind)
            if actual != expected:
                raise ValueError(f"reviewed expectation differs from official oracle: {rule['rule_id']}/{kind}/01")
            cases.append({"rule_id": rule["rule_id"], "case_kind": kind, "case_id": "01",
                          "expected_issue_count": sum(expected.values())})
    return {"schema_version": 1, "upstream_sha": manifest["upstream_sha"],
            "review": manifest["review"], "fixture_cross_check": "passed",
            "candidate_check": "not_performed", "cases": cases}


def check_scoreboard(report: dict, scoreboard: dict) -> None:
    expected = {(case["rule_id"], case["case_kind"], case["case_id"]): case for case in report["cases"]}
    seen = set()
    for case in scoreboard["cases"]:
        key = (case["rule_id"], case["case_kind"], case["case_id"])
        if key not in expected or key in seen or case["scope"] != "Published":
            raise ValueError(f"unexpected or duplicate pilot case: {key}")
        seen.add(key)
        count = expected[key]["expected_issue_count"]
        if (case["bucket"] != "supported_match" or case.get("scoring_normalizations")
                or case.get("scoring_policy") != "strict_identity"
                or case.get("official_issue_count") != count or case.get("candidate_issue_count") != count
                or case.get("missing_count", 0) or case.get("extra_count", 0)
                or case.get("missing") or case.get("extra")):
            raise ValueError(f"pilot candidate differs from reviewed expectations: {key}")
    if seen != expected.keys():
        raise ValueError("pilot scoreboard is incomplete")
    report["candidate_check"] = "passed"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--upstream-root", type=Path, required=True)
    parser.add_argument("--subset-root", type=Path, help="Create a new subset directory after source checks")
    parser.add_argument("--scoreboard", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    manifest = load_manifest(args.manifest)
    report = check_sources(manifest, args.upstream_root)
    if args.scoreboard:
        check_scoreboard(report, json.loads(args.scoreboard.read_text(encoding="utf-8")))
    if args.subset_root:
        args.subset_root.mkdir(parents=True, exist_ok=False)
        for rule in manifest["rules"]:
            relative = Path("Published") / rule["rule_id"]
            shutil.copytree(args.upstream_root / relative, args.subset_root / relative)
    report["manifest_sha256"] = hashlib.sha256(args.manifest.read_bytes()).hexdigest()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Pilot: {len(report['cases'])} fixtures checked; candidate={report['candidate_check']}; human approval=pending")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
