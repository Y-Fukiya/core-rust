"""Replay the frozen candidate skip inventory with a verified official CORE."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import tempfile

import open_rules_official_audit as audit
from review_accepted_open_rules import FIXTURE, load, replay


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "profile", "archive", "plan", "out"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--only", type=Path, help="manifest containing new_cases to replay")
    args = parser.parse_args()
    raw, out = args.candidate.resolve(), args.out.resolve()
    plan = load(args.plan)
    cases = [case for phase in ("unsupported", "deferred") for case in plan[phase]]
    if (len(plan["unsupported"]), len(plan["deferred"])) != (55, 49):
        raise ValueError("expected the frozen 55 unsupported and 49 deferred cases")
    if len(set(cases)) != len(cases) or not all(FIXTURE.fullmatch(case) for case in cases):
        raise ValueError("duplicate or invalid case keys")
    if args.only:
        selected = load(args.only)["new_cases"]
        if not selected or len(set(selected)) != len(selected) or set(selected) - set(cases):
            raise ValueError("selected cases must be a nonempty subset of the frozen inventory")
        plan = {phase: [case for case in group if case in selected] for phase, group in plan.items()}
    out.mkdir(parents=True)
    audit.check_platform(out, raw)
    profile = audit.load_profile(args.profile)
    pins = profile["engine"]
    source = audit.check_rules(raw, profile["rules"])
    if audit.git(raw, "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=all"):
        raise ValueError("candidate checkout is not clean")
    summary = {"source": source, "engine": {}, "phases": {}}
    with tempfile.TemporaryDirectory(prefix="core-skips-", dir="/tmp") as temp:
        work = Path(temp)
        summary["engine"] = audit.install_engine(args.archive, work, pins)
        engine = work / "core"
        audit.reject_ambient_dotenv(engine)
        cache = audit.cache_manifest(engine / "resources/cache")
        audit.check_cache(cache, pins)
        summary["cache_tree_sha256"] = cache["tree_sha256"]
        for name in ("home", "tmp"):
            (work / name).mkdir()
        env = audit.isolated_environment(work)
        version = audit.execute([str(engine / "core"), "version"], engine, env, out / "version", 30)
        if version["returncode"] != 0 or Path(version["stdout"]).read_text().strip() != pins["version"]:
            raise ValueError("unexpected official CORE version")

        def run(case):
            return replay(raw, engine, env, out / "cases" / case, case)

        with ThreadPoolExecutor(max_workers=3) as pool:
            for phase in ("unsupported", "deferred"):
                rows = []
                for result in pool.map(run, plan[phase]):
                    rows.append(result)
                    print(phase, result["case"], result["outcome"], result.get("rows"), flush=True)
                counts = dict(Counter(row["outcome"] for row in rows))
                summary["phases"][phase] = {"outcomes": counts, "cases": rows}
                audit.write_json(out / "summary.json", summary)


if __name__ == "__main__":
    main()
