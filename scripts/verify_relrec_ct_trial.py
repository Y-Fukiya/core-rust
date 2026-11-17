"""Project and verify the trial-only official RELREC and CT findings."""

import argparse
from collections import Counter
import csv
import difflib
import io
import json
from pathlib import Path
import shutil

import open_rules_official_audit as audit


LABELS = {
    "relrec-negative-01": "Published/CORE-000901/negative/01",
    "relrec-positive-01": "Published/CORE-000901/positive/01",
    "ct-negative-01": "Published/CORE-000929/negative/01",
    "ct-negative-02": "Published/CORE-000929/negative/02",
    "ct-positive-01": "Published/CORE-000929/positive/01",
}
FIELDS = ("Dataset", "Record", "Variable", "Value", "USUBJID", "SEQ")


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def cell(value):
    return "" if value is None or value == "None" else str(value)


def projection(official, source_sha):
    receipt = load(official / "summary.json")
    if not receipt["audit_complete"] or receipt["rules"]["head"] != source_sha:
        raise ValueError("expected a complete official replay on the pinned rules")
    if receipt["engine_version"] != "1.0.0":
        raise ValueError("unexpected CORE version")
    cases = receipt["cases"]
    if len(cases) != len(LABELS) or {row["label"] for row in cases} != LABELS.keys():
        raise ValueError("unexpected official replay case set")
    for result in cases:
        key = LABELS[result["label"]]
        if key != f"Published/{result['fixture']}" or result["status"] not in {"SUCCESS", "ISSUE REPORTED"}:
            raise ValueError(f"did not execute {key} with an official result")
        folder = official / "cases" / result["label"]
        report = folder / "report.json"
        if (audit.sha256(report) != result["report"]["sha256"]
                or audit.sha256(folder / "report.csv") != result["csv_sha256"]):
            raise ValueError(f"official receipt digest differs for {key}")
        records = []
        for row in load(report)["Issue_Details"]:
            variables, values = row["variables"] or [""], row["values"] or [""]
            for variable, value in zip(variables, values, strict=True):
                records.append(tuple(cell(v) for v in (
                    row["dataset"], row["row"], variable, value, row["USUBJID"], row["SEQ"]
                )))
        if len(records) != result["csv_data_rows"]:
            raise ValueError(f"authoritative output count differs for {key}")
        yield key, result, records


def generate(args):
    root, raw, official = args.root, args.raw, args.official
    previous_run = root / "linux-44145379917"
    prior = load(previous_run / "default/scoreboard.json")
    manifest = load(root / "consent-reference-oracles.json")
    patch = (root / "consent-reference-oracles.patch").read_bytes()
    manifest["trial_buckets"] = {
        "/".join(case[k] for k in ("scope", "rule_id", "case_kind", "case_id")): case["bucket"]
        for case in prior["cases"]
    }
    manifest["new_cases"] = list(LABELS.values())
    for key, result, records in projection(official, manifest["source_sha"]):
        name = f"{key}/results/results.csv"
        subset = args.subset / key
        shutil.copytree(raw / key / "data", subset / "data", dirs_exist_ok=True)
        shutil.copyfile(raw / key.split("/")[0] / key.split("/")[1] / "rule.yml",
                        subset.parents[1] / "rule.yml")
        out = io.StringIO(newline="")
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(FIELDS)
        writer.writerows(records)
        replacement = out.getvalue().encode()
        path = raw / name
        present = path.is_file()
        original = path.read_bytes() if present else b""
        if replacement == original:
            raise ValueError(f"unexpected identical published projection: {key}")
        if not present:
            if key != LABELS["relrec-negative-01"]:
                raise ValueError(f"unexpected missing upstream oracle: {key}")
            patch += f"diff --git a/{name} b/{name}\nnew file mode 100644\n".encode()
        patch += b"".join(difflib.diff_bytes(
            difflib.unified_diff, original.splitlines(keepends=True), replacement.splitlines(keepends=True),
            f"a/{name}".encode() if present else b"/dev/null", f"b/{name}".encode(),
        ))
        local = args.subset / name
        local.parent.mkdir(parents=True, exist_ok=True)
        local.write_bytes(replacement)
        manifest["files"].append({
            "case": key, "file": name,
            "before_sha256": audit.sha256(path) if present else None,
            "after_sha256": audit.sha256(local),
            "official_json_sha256": result["report"]["sha256"],
            "official_csv_sha256": result["csv_sha256"],
            "rows": len(records),
            "disposition": "runtime_fixed" if present else "trial_only_new_official_oracle",
        })
    manifest["verified_cases"] = len(manifest["files"])
    if manifest["verified_cases"] != 48:
        raise ValueError(f"expected exactly 48 reviewed projections: {manifest['verified_cases']}")
    (root / "relrec-ct-oracles.patch").write_bytes(patch)
    audit.write_json(root / "relrec-ct-oracles.json", manifest)


def verify(args):
    from verify_candidate_open_rules_skip_fixes import verify_overlay, verify_scoreboards

    profile, manifest = load(args.profile), load(args.manifest)
    previous = load(args.previous)
    new_paths = {entry["file"] for entry in manifest["files"] if entry["before_sha256"] is None}
    if new_paths != {f"{LABELS['relrec-negative-01']}/results/results.csv"}:
        raise ValueError("unexpected new oracle paths")
    for path in new_paths:
        if (args.raw / path).exists():
            raise ValueError(f"new oracle already exists in upstream: {path}")
    existing = {**manifest, "files": [entry for entry in manifest["files"] if entry["file"] not in new_paths]}
    verify_overlay(args.raw, args.proposal, profile, previous, existing)
    untracked = audit.git(args.proposal, "ls-files", "--others", "--exclude-standard", "-z")
    if set(untracked.decode().rstrip("\0").split("\0")) != new_paths:
        raise ValueError("unexpected trial-only paths")
    entries = {entry["case"]: entry for entry in manifest["files"]}
    for key, result, records in projection(args.official, manifest["source_sha"]):
        target = args.proposal / entries[key]["file"]
        with target.open(encoding="utf-8", newline="") as stream:
            expected = Counter(tuple(row[field] for field in FIELDS) for row in csv.DictReader(stream))
        if (expected != Counter(records) or audit.sha256(target) != entries[key]["after_sha256"]
                or len(records) != entries[key]["rows"]):
            raise ValueError(f"fresh official findings differ: {key}")
        print(key, result["status"], len(records))
    if args.default:
        verify_scoreboards(load(args.default), load(args.strict), manifest)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    build = modes.add_parser("generate")
    for arg in ("root", "raw", "official", "subset"):
        build.add_argument(f"--{arg}", type=Path, required=True)
    check = modes.add_parser("verify")
    for arg in ("raw", "proposal", "profile", "manifest", "previous", "official"):
        check.add_argument(f"--{arg}", type=lambda path: Path(path).resolve(), required=True)
    check.add_argument("--default", type=Path)
    check.add_argument("--strict", type=Path)
    args = parser.parse_args()
    (generate if args.mode == "generate" else verify)(args)
