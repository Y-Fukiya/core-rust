"""Replay proposed fixture profiles without modifying the adopted corpus."""

import argparse
from collections import Counter
import csv
import json
from pathlib import Path
import shutil
import tempfile

import open_rules_official_audit as audit
from review_accepted_open_rules import load, replay, verify_oracle_with_locators


def locators(report):
    def cell(value):
        return "" if value in (None, "None") else str(value)

    return Counter(
        (detail["dataset"], cell(detail["row"]), variable,
         cell(detail["USUBJID"]), cell(detail["SEQ"]))
        for detail in load(report / "report.json")["Issue_Details"]
        for variable in detail["variables"]
    )


def verify_candidate_skips(candidate, out, bundle, env):
    cases = {
        "Published/CORE-000039/negative/01": ("ISSUE REPORTED", 24),
        "Published/CORE-000039/positive/01": ("SUCCESS", 0),
        "Published/CORE-000039/positive/02": ("SKIPPED", 0),
        "Published/CORE-000660/negative/01": ("ISSUE REPORTED", 24),
        "Published/CORE-000660/positive/01": ("SUCCESS", 0),
        "Published/CORE-000660/positive/02": ("SUCCESS", 0),
        "Published/CORE-000660/positive/03": ("SKIPPED", 0),
    }
    records = []
    for case, expected in cases.items():
        record = replay(candidate, bundle, env, out / case, case)
        if (record.get("status"), record.get("rows")) != expected:
            raise ValueError(f"unexpected official candidate status or row count: {record}")
        records.append(record)
    audit.write_json(out / "summary.json", {"cases": records})


def prepare(raw, subset, archive, engine_profile, plan, frozen, out, candidate=None):
    out.mkdir(parents=True)
    audit.check_platform(out, raw)
    rules = audit.check_rules(raw, plan["rules"])
    if audit.git(raw, "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=all"):
        raise ValueError("raw checkout is not clean")
    profile = audit.load_profile(engine_profile)
    engine = profile["engine"]
    if candidate is not None:
        audit.check_rules(candidate, profile["rules"])
        if audit.git(candidate, "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=all"):
            raise ValueError("candidate checkout is not clean")
    cases = frozen["cases"]
    if set(cases) - set(plan["new_skips"]):
        raise ValueError("profile repairs must be among the reviewed applicability skips")
    original_skips, repairs = [], []
    with tempfile.TemporaryDirectory(prefix="core-profiles-", dir="/tmp") as temporary:
        work = Path(temporary)
        evidence = audit.install_engine(archive, work, engine)
        bundle = work / "core"
        audit.reject_ambient_dotenv(bundle)
        cache = audit.cache_manifest(bundle / "resources/cache")
        audit.check_cache(cache, engine)
        for directory in ("home", "tmp"):
            (work / directory).mkdir()
        env = audit.isolated_environment(work)
        version = audit.execute([str(bundle / "core"), "version"], bundle, env, out / "version", 30)
        if version["returncode"] != 0 or Path(version["stdout"]).read_text().strip() != engine["version"]:
            raise ValueError("unexpected official CORE version")
        if candidate is not None:
            verify_candidate_skips(candidate, out / "candidate", bundle, env)
        for case in plan["new_skips"]:
            original = replay(raw, bundle, env, out / "original" / case, case)
            if original["outcome"] != "rule_not_selected":
                raise ValueError(f"expected the original profile to remain inapplicable: {case}")
            original_skips.append(case)
        for case, expected in cases.items():
            rule = "/".join(case.split("/")[:2]) + "/rule.yml"
            inputs = audit.tracked_files(raw, f"{case}/data")
            inputs.update(audit.tracked_files(raw, rule))
            for path in inputs:
                destination = subset / path
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(raw / path, destination)
            dotenv = subset / case / "data" / ".env"
            profile = audit.read_fixture_environment(dotenv)
            if any(profile[key] != value for key, value in expected["before"].items()):
                raise ValueError(f"unexpected authored profile: {case}")
            profile.update(expected["after"])
            dotenv.write_text("".join(f"{key}={value}\n" for key, value in profile.items()), encoding="utf-8")
            report = out / "repaired" / case
            observation = replay(raw, bundle, env, report, case, data_override=dotenv.parent)
            if observation.get("status") != "ISSUE REPORTED":
                raise ValueError(f"repaired profile did not report issues: {observation}")
            identity = locators(report)
            if identity != Counter(map(tuple, expected["locators"])):
                raise ValueError(f"new profile did not reproduce the frozen official identity: {case}")
            by_row = {(dataset, row, variable): (subject, seq) for dataset, row, variable, subject, seq in identity}
            if len(by_row) != sum(identity.values()):
                raise ValueError(f"ambiguous CSV/JSON locator mapping: {case}")
            target = subset / case / "results" / "results.csv"
            target.parent.mkdir()
            source = raw / case / "results" / "results.csv"
            audit.tracked_files(raw, source.relative_to(raw).as_posix())
            with (report / "report.csv").open(encoding="utf-8-sig", newline="") as inp, \
                 target.open("w", encoding="utf-8", newline="") as output:
                reader = csv.reader(inp)
                writer = csv.writer(output, lineterminator="\n")
                writer.writerow([*next(reader), "USUBJID", "SEQ"])
                for row in reader:
                    writer.writerow([*row, *by_row[tuple(row[:3])]])
            if not verify_oracle_with_locators(report, target):
                raise ValueError(f"generated CSV does not match same-run official reports: {case}")
            repairs.append({
                "case": case, "rows": observation["rows"],
                "files": {
                    path.relative_to(subset).as_posix(): {
                        "before_sha256": audit.sha256(raw / path.relative_to(subset)),
                        "after_sha256": audit.sha256(path),
                    }
                    for path in (dotenv, target)
                },
            })
    audit.write_json(out / "profile-repairs.json", {
        "rules": rules, "engine": evidence, "prior_control_run": frozen["source_run"],
        "original_rule_not_selected": original_skips, "repairs": repairs,
    })


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for argument in ("raw", "subset", "archive", "engine-profile", "plan", "frozen", "out"):
        parser.add_argument(f"--{argument}", type=Path, required=True)
    parser.add_argument("--candidate", type=Path)
    args = parser.parse_args()
    prepare(args.raw.resolve(), args.subset.resolve(), args.archive.resolve(),
            args.engine_profile, load(args.plan), json.loads(args.frozen.read_text()), args.out.resolve(),
            args.candidate.resolve() if args.candidate else None)
