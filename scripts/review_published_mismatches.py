"""Replay published and trial inputs independently on pinned CORE."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import csv
from pathlib import Path
import shutil
import tempfile

import open_rules_official_audit as audit
from review_accepted_open_rules import csv_records, load, replay
from verify_candidate_open_rules_skip_fixes import verify_overlay


SUPPORTED = ((132, "negative"), (308, "negative"), (457, "positive"),
             (517, "negative"), (783, "negative"), (914, "negative"))
DEFERRED = ((239, "negative"), (560, "negative"), (690, "positive"),
            (699, "negative"), (711, "negative"), (714, "negative"), (866, "negative"))
CONTROL = "Published/CORE-000560/positive/01"
RECOVERED_HEADER_CASE = "Published/CORE-000457/positive/01"
KEYS = [f"Published/CORE-{rule:06d}/{kind}/01" for rule, kind in SUPPORTED + DEFERRED]
MODES = ("raw", "trial")
EXECUTED = {"success", "issue_reported"}
INAPPLICABLE = {
    "Published/CORE-000478/negative/01",
    "Published/CORE-000478/positive/01",
    "Published/CORE-000606/positive/03",
    "Published/CORE-000712/positive/03",
    "Published/CORE-000660/positive/03",
}


def locator(value):
    return "" if value is None or value == "None" else str(value)


def project(receipt, source, output, report):
    key = receipt["case"]
    if receipt["outcome"] not in EXECUTED:
        return
    filename = output / key / "results/results.csv"
    (output / key).mkdir(parents=True)
    shutil.copytree(source / key / "data", output / key / "data")
    rule = Path(*Path(key).parts[:2]) / "rule.yml"
    shutil.copyfile(source / rule, output / rule)
    if audit.sha256(report) != receipt["json_sha256"]:
        raise ValueError(f"official JSON does not match receipt: {key}")
    filename.parent.mkdir()
    with filename.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(("Dataset", "Record", "Variable", "Value", "USUBJID", "SEQ"))
        rows = 0
        for issue in load(report)["Issue_Details"]:
            for variable, value in zip(issue["variables"] or [""], issue["values"] or [""], strict=True):
                writer.writerow((issue["dataset"], locator(issue["row"]), variable, value,
                                 locator(issue["USUBJID"]), locator(issue["SEQ"])))
                rows += 1
        if rows != receipt["rows"]:
            raise ValueError(f"official JSON and report CSV row counts differ: {key}")


def overlay(args, profile):
    manifest = load(args.trial_manifest)
    known_new = "Published/CORE-000901/negative/01/results/results.csv"
    new = [row for row in manifest["files"] if row["before_sha256"] is None]
    if len(new) != 1 or new[0]["file"] != known_new:
        raise ValueError("unexpected trial-only CSV")
    old = dict(manifest, files=[row for row in manifest["files"] if row not in new])
    verify_overlay(args.raw, args.trial, profile, load(args.older_manifest), old)
    untracked = audit.git(args.trial, "ls-files", "--others", "--exclude-standard", "-z")
    if (untracked.decode() != known_new + "\0"
            or audit.sha256(args.trial / known_new) != new[0]["after_sha256"]):
        raise ValueError("unexpected new file in trial checkout")
    return manifest


def run_official(args):
    args.out.mkdir(parents=True, exist_ok=True)
    audit.check_platform(args.out, args.raw)
    profile = audit.load_profile(args.profile)
    pins = profile["engine"]
    overlay(args, profile)
    source = audit.check_rules(args.raw, profile["rules"])
    if audit.git(args.raw, "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=all"):
        raise ValueError("published checkout contains unpublished changes")
    summary = {"source": source, "cases": [], "engine": {}, "engine_version": None}
    with tempfile.TemporaryDirectory(prefix="core-13-", dir="/tmp") as folder:
        work = Path(folder)
        summary["engine"] = audit.install_engine(args.archive, work, pins)
        engine = work / "core"
        audit.reject_ambient_dotenv(engine)
        cache = audit.cache_manifest(engine / "resources/cache")
        audit.check_cache(cache, pins)
        summary["cache_tree_sha256"] = cache["tree_sha256"]
        for name in ("home", "tmp"):
            (work / name).mkdir()
        env = audit.isolated_environment(work)
        version = audit.execute([str(engine / "core"), "version"], engine, env, args.out / "version", 30)
        summary["engine_version"] = Path(version["stdout"]).read_text().strip()
        if version["returncode"] or summary["engine_version"] != pins["version"]:
            raise ValueError("unexpected official CORE version")

        def paired(key):
            result = {"case": key}
            for name in MODES:
                source = getattr(args, name)
                receipt_dir = args.out / "receipts" / name / key
                observation = replay(args.raw, engine, env, receipt_dir, key, data_override=source / key / "data")
                result[name] = observation
                project(observation, source, args.scratch / name, receipt_dir / "report.json")
                expected = source / key / "results/results.csv"
                if observation["outcome"] in EXECUTED and expected.exists():
                    original = csv_records(expected)
                    official = csv_records(receipt_dir / "report.csv")
                    result[name]["published_csv_matches_official"] = original == official
            return result

        with ThreadPoolExecutor(max_workers=3) as pool:
            for phase in (KEYS[:len(SUPPORTED)], KEYS[len(SUPPORTED):] + [CONTROL]):
                for pair in pool.map(paired, phase):
                    print(pair["case"], *[(pair[mode]["outcome"], pair[mode].get("rows")) for mode in MODES], flush=True)
                    summary["cases"].append(pair)
                    audit.write_json(args.out / "official.json", summary)
        audit.check_cache(audit.cache_manifest(engine / "resources/cache"), pins)
        audit.check_rules(args.raw, profile["rules"])


def read_score(path):
    summary = load(path)
    keys = ("scope", "rule_id", "case_kind", "case_id")
    by_key = {"/".join(row[k] for k in keys): row for row in summary["cases"]}
    if len(by_key) != len(summary["cases"]):
        raise ValueError(f"duplicate scoreboard keys: {path}")
    return summary["summary"], by_key


def report(args):
    official = load(args.out / "official.json")
    if [pair["case"] for pair in official["cases"]] != KEYS + [CONTROL]:
        raise ValueError("missing or reordered replay results")
    board = lambda directory: read_score(args.out / directory / "scoreboard.json")
    published, raw = board("raw-default")
    tested, trial = board("trial-default")
    trial_strict, trial_strict_cases = board("trial-strict")
    strict_fresh = {mode: board(f"{mode}-fresh")[1] for mode in MODES}
    if any(len(cases) != 2298 for cases in (raw, trial, trial_strict_cases)):
        raise ValueError("full scoreboards do not have 2298 cases")
    if not raw.keys() == trial.keys() == trial_strict_cases.keys():
        raise ValueError("full scoreboards differ in case keys")
    rows = []
    for pair in official["cases"]:
        key = pair["case"]
        raw_run, trial_run = (pair[mode] for mode in MODES)
        if trial_run["outcome"] not in EXECUTED:
            category = "inapplicable_or_unconfirmed"
        elif strict_fresh["trial"][key]["bucket"] != "supported_match":
            category = "rust_or_projection_difference"
        elif key == RECOVERED_HEADER_CASE and strict_fresh["raw"].get(key, {}).get("bucket") == "supported_match":
            category = "fixture_and_rust_header_recovery"
        elif raw_run["effective_data_sha256"] != trial_run["effective_data_sha256"]:
            category = "fixture_or_profile"
        elif raw_run["outcome"] not in EXECUTED:
            category = "inapplicable_or_unconfirmed"
        elif strict_fresh["raw"][key]["bucket"] != "supported_match":
            category = "rust_or_projection_difference"
        elif not raw_run.get("published_csv_matches_official", True):
            category = "stale_expected_csv"
        else:
            category = "no_published_oracle_difference"
        row = {"case": key, "legacy_priority": "supported" if key in KEYS[:len(SUPPORTED)] else
               "profile_control" if key == CONTROL else "deferred", "category": category}
        for mode, full in (("raw", raw), ("trial", trial)):
            finding = strict_fresh[mode].get(key)
            row[mode] = {
                "official_outcome": pair[mode]["outcome"], "official_rows": pair[mode].get("rows"),
                "stored_bucket": full[key]["bucket"], "stored_expected_rows": full[key]["official_issue_count"],
                "candidate_rows": full[key]["candidate_issue_count"],
                "fresh_comparison": {k: finding[k] for k in ("bucket", "missing_count", "extra_count")}
                if finding else None,
            }
        rows.append(row)
    out = {"published": published, "trial_default": tested, "trial_strict": trial_strict,
           "counts": dict(Counter(row["category"] for row in rows if row["case"] != CONTROL)), "cases": rows}
    audit.write_json(args.out / "classification.json", out)
    for row in rows:
        print(row)
    for key in INAPPLICABLE:
        if any(cases[key]["bucket"] not in {"skipped_unsupported", "deferred_oracle_gap_skipped"}
               for cases in (raw, trial, trial_strict_cases)):
            raise ValueError(f"known inapplicable case counted as a match: {key}")
    if any(summary["harness_error"] for summary in (published, tested, trial_strict)):
        raise ValueError("full scoring had harness errors")
    if tested["supported_mismatch"] or tested["deferred_oracle_gap_mismatch"]:
        raise ValueError("trial full-corpus mismatches remain")
    recovery = next(row for row in rows if row["case"] == RECOVERED_HEADER_CASE)
    if (recovery["category"] != "fixture_and_rust_header_recovery"
            or (recovery["raw"]["official_rows"], recovery["trial"]["official_rows"]) != (2, 0)):
        raise ValueError("CORE-000457 must match official CORE before and after header repair")
    if any(row["category"] in {"rust_or_projection_difference", "inapplicable_or_unconfirmed"}
           for row in rows):
        raise ValueError("a reviewed case still lacks a verified disposition")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    run = sub.add_parser("replay")
    run_args = ("raw", "trial", "profile", "archive", "out", "scratch", "older-manifest", "trial-manifest")
    for arg in run_args:
        run.add_argument(f"--{arg}", type=lambda path: Path(path).resolve(), required=True)
    sub.add_parser("report").add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    (run_official if args.action == "replay" else report)(args)
