"""Verify a proposed accepted-oracle snapshot. Never replace the acceptance gate."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import csv
import json
from pathlib import Path
import re
import tempfile

import open_rules_official_audit as audit


FIXTURE = re.compile(r"Published/(CORE-\d{6})/(?:positive|negative)/\d+")
PROFILE_REPAIR = "Published/CORE-000560/negative/01/data/.env"
PASS_STATUSES = {"ISSUE REPORTED", "SUCCESS"}
NO_SELECTION = "No rules were selected for this standard/version"
MISSING_VERSION = "Missing option '-v' / '--version'"


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def csv_records(path):
    with path.open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.reader(stream)
        return next(reader), Counter(tuple(row) for row in reader)


def check_proposal(raw, proposed, manifest):
    files = {entry["file"]: entry for entry in manifest["files"]}
    if len(files) != len(manifest["files"]):
        raise ValueError("duplicate proposal paths")
    for path, hashes in files.items():
        fixture, _, name = path.rpartition("/")
        if path != PROFILE_REPAIR and (name != "results.csv" or not fixture.endswith("/results")
                                      or not FIXTURE.fullmatch(fixture.removesuffix("/results"))):
            raise ValueError(f"not an approved proposal path: {path}")
        if audit.sha256(raw / path) != hashes["before_sha256"]:
            raise ValueError(f"original hash does not match: {path}")
        if audit.sha256(proposed / path) != hashes["after_sha256"]:
            raise ValueError(f"proposal hash does not match: {path}")
    observed = set(audit.git(proposed, "diff", "--name-only", "-z", "HEAD", "--").decode().strip("\0").split("\0"))
    if observed != set(files):
        raise ValueError(f"proposal affects unexpected paths: {sorted(observed ^ set(files))}")


def replay(raw, engine, env, dest, case, version=None):
    rule_id = FIXTURE.fullmatch(case).group(1)
    first = raw / case / "data"
    rule = raw / "Published" / rule_id / "rule.yml"
    files = audit.tracked_files(raw, f"{case}/data")
    files.update(audit.tracked_files(raw, f"Published/{rule_id}/rule.yml"))
    dest.mkdir(parents=True)
    cmd = [
        str(engine / "core"), "validate", "-ca", str(engine / "resources/cache"), "-ps", "1",
        "-lr", str(rule), "-d", str(first), "-dep", str(first / ".env"),
        "-of", "CSV", "-of", "JSON", "-o", str(dest / "report"), "-p", "disabled", "-l", "info",
    ]
    if version:
        cmd += ["-v", version]
    process = audit.execute(cmd, engine, env, dest / "engine", 120)
    for path, metadata in files.items():
        if audit.sha256(raw / path) != metadata["sha256"]:
            raise ValueError(f"input changed during replay: {path}")
    exit_code = process["returncode"]
    obs = {"case": case, "version_override": version, "exit": exit_code, "inputs": files}
    json_path, csv_path = dest / "report.json", dest / "report.csv"
    stderr = (dest / "engine.stderr.txt").read_text(errors="replace")
    if process["timed_out"]:
        obs["outcome"] = "timeout"
    elif exit_code != 0:
        obs["outcome"] = "process_error"
        if not json_path.exists() and not csv_path.exists():
            if exit_code == 1 and NO_SELECTION in stderr:
                obs["outcome"] = "rule_not_selected"
            elif exit_code == 2 and MISSING_VERSION in stderr:
                obs["outcome"] = "missing_version"
    else:
        try:
            info = audit.inspect_report(json_path, rule_id)
            status = info["status"]
            obs["rows"] = audit.verify_csv_projection(csv_path, json_path, status)
            obs.update(status=status, outcome=status.lower().replace(" ", "_"),
                       json_sha256=info["sha256"], csv_sha256=audit.sha256(csv_path))
        except (OSError, ValueError, KeyError, TypeError) as error:
            obs.update(outcome="invalid_report", error=str(error))
    audit.write_json(dest / "observation.json", obs)
    return obs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for argument in ("raw", "proposed", "plan", "proposal-manifest", "engine-profile", "archive", "out"):
        parser.add_argument(f"--{argument}", required=True, type=Path)
    args = parser.parse_args()
    raw, proposed, out = (value.resolve() for value in (args.raw, args.proposed, args.out))
    manifest = load(args.proposal_manifest)
    plan = load(args.plan)
    reference = plan["cases"]
    if len(reference) != len(set(reference)) or any(not FIXTURE.fullmatch(case) for case in reference):
        raise ValueError("invalid or duplicate plan cases")
    pins = plan["rules"]
    if pins["sha"] != manifest["source_sha"]:
        raise ValueError("review plan and proposal pins differ")
    engine = audit.load_profile(args.engine_profile)["engine"]
    out.mkdir(parents=True)
    audit.check_platform(out, raw)
    audit.check_rules(raw, pins)
    if audit.git(raw, "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=all"):
        raise ValueError("raw checkout contains untracked inputs")
    if audit.git(proposed, "rev-parse", "HEAD").decode().strip() != pins["sha"]:
        raise ValueError("proposal checkout has the wrong base")
    check_proposal(raw, proposed, manifest)
    with tempfile.TemporaryDirectory(prefix="core-accepted-", dir="/tmp") as temporary:
        work = Path(temporary)
        evidence = audit.install_engine(args.archive, work, engine)
        bundle = work / "core"
        audit.reject_ambient_dotenv(bundle)
        cache = audit.cache_manifest(bundle / "resources/cache")
        audit.check_cache(cache, engine)
        env = audit.isolated_environment(work)
        (work / "home").mkdir()
        (work / "tmp").mkdir()
        version = audit.execute([str(bundle / "core"), "version"], bundle, env, out / "version", 30)
        if version["returncode"] != 0 or Path(version["stdout"]).read_text().strip() != engine["version"]:
            raise ValueError("unexpected official CORE version")
        def run(case):
            return replay(raw, bundle, env, out / "raw" / case, case)
        with ThreadPoolExecutor(max_workers=3) as pool:
            raw_cases = {obs["case"]: obs for obs in pool.map(run, reference)}
        adsl = PROFILE_REPAIR.removesuffix("/data/.env")
        controls = [
            replay(raw, bundle, env, out / "version-controls" / version, adsl, version)
            for version in ("1-0", "1-1", "1-2", "1-3")
        ]
        checks = []
        for entry in manifest["files"]:
            if not entry["file"].endswith("/results/results.csv"):
                continue
            case = entry["file"].removesuffix("/results/results.csv")
            src = out / "raw" / case / "report.csv"
            record = raw_cases[case]
            if case == adsl:
                src, record = out / "version-controls/1-3/report.csv", controls[-1]
            match = (record.get("status") in PASS_STATUSES
                     and csv_records(src) == csv_records(proposed / entry["file"]))
            checks.append({"case": case, "status": record.get("status"), "verified": match})
        scopes = {case: raw_cases[case]["outcome"] for case in plan["new_skips"]}
        adsl_profile_verified = (raw_cases[adsl]["outcome"] == "missing_version"
                                 and all(control.get("status") == "ISSUE REPORTED" for control in controls)
                                 and (proposed / PROFILE_REPAIR).read_bytes()
                                 == (raw / PROFILE_REPAIR).read_bytes().replace(b"VERSION=", b"VERSION=1-3", 1)
                                 and all(csv_records(out / "version-controls" / control["version_override"] / "report.csv")
                                         == csv_records(out / "version-controls/1-3/report.csv") for control in controls))
        audit.check_rules(raw, pins)
        audit.check_cache(audit.cache_manifest(bundle / "resources/cache"), engine)
    result = {
        "adopted_sha": pins["sha"], "runtime": evidence, "conformance_claim": False,
        "cases": raw_cases, "counts": dict(Counter(r["outcome"] for r in raw_cases.values())),
        "proposal_checks": checks, "new_skip_controls": scopes, "adam_version_controls": controls,
        "adam_profile_verified": adsl_profile_verified,
    }
    audit.write_json(out / "summary.json", result)
    fail = (len(checks) != manifest["cases"] or not all(c["verified"] for c in checks)
            or not all(outcome == "rule_not_selected" for outcome in scopes.values())
            or not adsl_profile_verified)
    return int(fail)


if __name__ == "__main__":
    raise SystemExit(main())
