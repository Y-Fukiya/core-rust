"""Verify the candidate overlays and read back skip and issue counts."""

import argparse
from collections import Counter
import csv
from pathlib import Path

import open_rules_official_audit as audit
from review_accepted_open_rules import load


def verify_overlay(raw, proposal, profile, previous, manifest):
    audit.check_rules(raw, profile["rules"])
    if manifest["source_sha"] != profile["rules"]["sha"]:
        raise ValueError("candidate oracle manifest has the wrong base")
    files = [*previous["files"], *manifest["files"]]
    names = {entry["file"] for entry in files}
    observed = set(audit.git(proposal, "diff", "--name-only", "-z", "HEAD").decode().strip("\0").split("\0"))
    if len(names) != len(files) or observed != names:
        raise ValueError(f"unexpected or overlapping overlay paths: {sorted(names ^ observed)}")
    for entry in files:
        path = entry["file"]
        if (audit.sha256(raw / path) != entry["before_sha256"]
                or audit.sha256(proposal / path) != entry["after_sha256"]):
            raise ValueError(f"unexpected overlay digest: {path}")


def verify_scoreboards(default, strict, manifest):
    def by_key(scoreboard):
        cases = scoreboard["cases"]
        keys = {"/".join(case[k] for k in ("scope", "rule_id", "case_kind", "case_id")): case for case in cases}
        if len(keys) != len(cases):
            raise ValueError("scoreboard has duplicate case keys")
        return keys

    targets = {entry["case"]: entry for entry in manifest["files"]}
    if len(targets) != manifest.get("verified_cases", 24) or set(targets) - manifest["trial_buckets"].keys():
        raise ValueError("unexpected target set")
    if any(board["upstream"]["observed_sha"] != manifest["source_sha"] for board in (default, strict)):
        raise ValueError("scoreboards have the wrong corpus SHA")
    actual = by_key(default)
    expected = manifest["trial_buckets"] | dict.fromkeys(targets, "supported_match")
    changed = {key: (expected.get(key), actual.get(key, {}).get("bucket"))
               for key in sorted(expected.keys() | actual.keys())
               if expected.get(key) != actual.get(key, {}).get("bucket")}
    if changed:
        raise ValueError(f"unexpected bucket changes: {changed}")
    strict_cases = by_key(strict)
    for key, expected in targets.items():
        entry = strict_cases[key]
        observed = (entry["bucket"], entry["missing_count"], entry["extra_count"],
                    entry["official_issue_count"], entry["candidate_issue_count"])
        if observed != ("supported_match", 0, 0, expected["rows"], expected["rows"]):
            raise ValueError(f"strict oracle differs for {key}: {observed}")
    for board in (default, strict):
        print({key: board["summary"][key] for key in ("total_cases", "supported_match", "supported_mismatch",
              "deferred_oracle_gap_mismatch", "skipped_unsupported", "deferred_oracle_gap_skipped", "harness_error")})


def verify_fresh_official(official, proposal, manifest):
    receipts = load(official / "summary.json")
    if receipts["source"]["head"] != manifest["source_sha"]:
        raise ValueError("official replay has the wrong rules SHA")
    cases = {row["case"]: row for phase in receipts["phases"].values() for row in phase["cases"]}
    names = set(manifest["new_cases"])
    if len(names) != 8 or set(cases) != names:
        raise ValueError("unexpected fresh official case set")

    def cell(value):
        return "" if value is None else str(value)

    def locator(value):
        return "" if value == "None" else cell(value)

    for key, receipt in cases.items():
        path = official / "cases" / key / "report.json"
        if receipt["outcome"] not in {"issue_reported", "success"} or audit.sha256(path) != receipt["json_sha256"]:
            raise ValueError(f"official CORE did not confirm an executed result: {key}")
        observed = Counter(
            (detail["dataset"], cell(detail["row"]), variable, locator(detail["USUBJID"]), locator(detail["SEQ"]))
            for detail in load(path)["Issue_Details"]
            for variable in detail["variables"] or [""]
        )
        fields = ("Dataset", "Record", "Variable", "USUBJID", "SEQ")
        with (proposal / key / "results/results.csv").open(encoding="utf-8", newline="") as stream:
            expected = Counter(tuple(row[field] for field in fields) for row in csv.DictReader(stream))
        if observed != expected or sum(observed.values()) != receipt["rows"]:
            raise ValueError(f"fresh official finding identity differs: {key}")
        print(key, receipt["status"], receipt["rows"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    commands = parser.add_subparsers(dest="command", required=True)
    overlay = commands.add_parser("overlay")
    for name in ("raw", "proposal", "profile", "previous"):
        overlay.add_argument(f"--{name}", required=True, type=Path)
    scores = commands.add_parser("scores")
    for name in ("default", "strict"):
        scores.add_argument(f"--{name}", required=True, type=Path)
    official = commands.add_parser("official")
    official.add_argument("--receipts", required=True, type=Path)
    official.add_argument("--proposal", required=True, type=Path)
    args = parser.parse_args()
    manifest = load(args.manifest)
    if args.command == "overlay":
        verify_overlay(args.raw.resolve(), args.proposal.resolve(), load(args.profile), load(args.previous), manifest)
    elif args.command == "official":
        verify_fresh_official(args.receipts, args.proposal, manifest)
    else:
        verify_scoreboards(load(args.default), load(args.strict), manifest)


if __name__ == "__main__":
    main()
