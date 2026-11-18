"""Run the locked LB comparison as fresh CLI processes, one at a time."""

import argparse
import csv
import hashlib
import json
import os
import statistics
import subprocess
import sys
import tarfile
import time
from collections import Counter
from decimal import Decimal
from pathlib import Path


IDS = ("CORE-000289", "CORE-000290", "CORE-000298", "CORE-000299", "CORE-002098")
SQL_COMMIT = "4a734105540a83f1df80d061df5049bd4b6bda4d"
RULE_TAR = "bac26a001ab45f60b5f6b9f7d2f41a71506f10ac6ec41cfc2df1fee381dece30"
RUST_COMMIT = "6ce2e19c4dbce6cef50dcadf9bfade0e83ae6c93"
MEASURED_BINARY = "1271adaf03b0755115a4a2785ceca809b0a4dc762352bf3410343543cbf0721b"


def digest(path):
    with path.open("rb") as reader:
        return hashlib.file_digest(reader, "sha256").hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(path):
    return json.loads(path.read_bytes())


def dump(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n")


def stage_rules(archive, dest):
    require(digest(archive) == RULE_TAR, "unrecognized contribution archive")
    require(not dest.exists(), "expected a new rules directory")
    dest.mkdir(mode=0o700)
    extracted = {}
    with tarfile.open(archive, "r|gz") as tar:
        for member in tar:
            parts = Path(member.name).parts
            if (len(parts) >= 3 and "/sdtm_rules/" in member.name and parts[-2] in IDS
                    and member.isfile() and parts[-1].endswith((".yml", ".yaml"))):
                rid = parts[-2]
                require(rid not in extracted, f"multiple YAMLs for {rid}")
                path = dest / (rid + ".yml")
                path.write_bytes(tar.extractfile(member).read())
                extracted[rid] = {"source": member.name, "sha256": digest(path)}
    require(set(extracted) == set(IDS), "rule set differs")
    return extracted


def issue_key(rule, ds, row, variables, subject, seq):
    numeric_row = Decimal(str(row))
    require(numeric_row == numeric_row.to_integral_value(), "fractional record number")
    return (rule, ds, int(numeric_row), tuple(sorted(variables)), subject, str(seq))


def inspect(engine, folder, rows):
    if engine == "sql":
        report = read_json(folder / "sql.json")
        details = report["Conformance_Details"]
        require((details["CORE_Engine_Version"], details["Standard"], details["Version"])
                == ("1.0.0", "SDTMIG", "V3.4"), "unexpected SQL engine or standard")
        require([(d["filename"], d["length"]) for d in report["Dataset_Details"]]
                == [("LB", rows)], "SQL loaded an unexpected dataset or row count")
        require(sorted((r["core_id"], r["status"], r["skip_reason"]) for r in report["Rules_Report"])
                == [(rid, "ISSUE REPORTED", "") for rid in IDS], "SQL rule did not execute as expected")
        require(sorted((r["core_id"], r["dataset"], r["issues"]) for r in report["Issue_Summary"])
                == [(rid, "LB", 20) for rid in IDS], "unexpected SQL finding totals")
        return Counter(issue_key(r["core_id"], r["dataset"], r["row"], r["variables"], r["USUBJID"], r["SEQ"])
                       for r in report["Issue_Details"])
    report = read_json(folder / "rust" / "report.json")
    meta = report["metadata"]
    require((meta["engine"], meta["standard"], meta["standard_version"], meta["dataset_count"])
            == ("core-rs", "sdtmig", "3.4", 1), "unexpected Rust engine, standard or dataset count")
    require(sorted((r["rule_id"], r["dataset"], r["domain"], r["execution_status"],
                    r["error_count"], len(r["errors"])) for r in report["results"])
            == [(rid, "LB", "LB", "failed", 20, 20) for rid in IDS],
            "Rust rule did not execute as expected")
    require(report["summary"] == dict(total_results=5, passed=0, failed=5, skipped=0, error_count=100),
            "unexpected Rust finding totals")
    return Counter(issue_key(r["rule_id"], r["dataset"], r["row"], r["variables"], r["usubjid"], r["seq"])
                   for result in report["results"] for r in result["errors"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("bench", "bundle", "input", "rule-archive", "db-tmp"):
        parser.add_argument("--" + option, type=lambda value: Path(value).resolve(), required=True)
    parser.add_argument("--rows", type=int, required=True)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--label", required=True)
    parser.add_argument("--allow-rebuild", action="store_true",
                        help="Allow a release-verified rebuild of the same Rust commit")
    args = parser.parse_args()
    require(args.label.isidentifier(), "run label must be a simple identifier")
    root = args.bench / args.label
    root.mkdir(mode=0o700)
    rules = root / "rules"
    rule_pins = stage_rules(args.rule_archive, rules)
    target = args.input / "lb.xpt"
    binary = args.bundle / "bin" / "core-rs"
    binary_hash = digest(binary)
    manifest = read_json(args.bundle / "release-manifest.json")
    require(manifest["git"] == {"available": True, "commit": RUST_COMMIT, "dirty": False},
            "bundle must come from the pinned clean Rust commit")
    require(manifest["artifacts"] == [{"path": "bin/core-rs", "sha256": binary_hash}],
            "Rust binary does not match its manifest")
    require(binary_hash == MEASURED_BINARY or args.allow_rebuild,
            "binary differs from the original measurement; verify the rebuild and use --allow-rebuild")
    pin = {"xpt_sha256": digest(target), "rows": args.rows, "xpt_bytes": target.stat().st_size,
           "expected_sha256": digest(args.input / "expected.csv"), "rules": rule_pins,
           "binary_sha256": binary_hash, "sql_commit": SQL_COMMIT, "rust_source": RUST_COMMIT,
           "sql_archive_sha256": "09772a78ed91d62d6f029c6abfaeee48fd68b72b6773673dbe8fd3d44fcaf949"}
    dump(root / "inputs.json", pin)
    with (args.input / "expected.csv").open(newline="") as reader:
        expected = Counter(issue_key(r["rule_id"], r["dataset"], r["row"], r["variables"].split("|"),
                                     r["usubjid"], r["seq"]) for r in csv.DictReader(reader))
    require(len(expected) == sum(expected.values()) == 100, "expected planted issue set")
    require(max(issue[2] for issue in expected) == args.rows, "tail sentinel is missing")
    env = os.environ.copy()
    for name in ("TMPDIR", "HOME"):
        directory = args.db_tmp if name == "TMPDIR" else root / "home"
        directory.mkdir(mode=0o700, exist_ok=True)
        env[name] = str(directory)
    sql_dir = args.bench / ("cdisc-rules-engine-" + SQL_COMMIT)
    measurements = []
    for i in range(args.repeats + 1):
        # Every sample launches a new CLI. SQL uses a fresh pgserver instance each time.
        order = ("sql", "rust") if i % 2 == 0 else ("rust", "sql")
        for engine in order:
            out = root / f"{i:02d}-{engine}"
            out.mkdir(mode=0o700)
            if engine == "sql":
                command = [sys.executable, "-B", "-E", "-s", "core.py", "validate",
                           "-mem", "-s", "sdtmig", "-v", "3-4", "-dp", target, "-lr", rules,
                           "-of", "JSON", "-o", out / "sql", "-p", "disabled"]
            else:
                command = [binary, "validate", "-s", "sdtmig", "-v", "3.4", "--dataset-path", target,
                           "--local-rules", *(rules / (rid + ".yml") for rid in IDS),
                           "--output-format", "json", "-o", out / "rust"]
            load = os.getloadavg()[0]
            with (out / "command.log").open("wb") as log:
                start = time.perf_counter()
                completed = subprocess.run(["/usr/bin/time", "-l", *command], env=env, cwd=sql_dir,
                                           stdout=log, stderr=subprocess.STDOUT, timeout=240)
                seconds = time.perf_counter() - start
            require(completed.returncode == 0, f"{engine} process failed; see {out.name}/command.log")
            found = inspect(engine, out, args.rows)
            require(found == expected,
                    f"{engine} findings differ: missing {sum((expected-found).values())}, extra {sum((found-expected).values())}")
            item = {"pair": i, "engine": engine, "wall_seconds": seconds, "load_1m_before": load,
                    "findings": sum(found.values()), "rules_executed": len(IDS), "skips": 0,
                    "expected_match": True}
            measurements.append(item)
            dump(root / "measurements.json", measurements)
            print(json.dumps(item), flush=True)
    require(digest(target) == pin["xpt_sha256"], "XPT changed during benchmark")
    require(all(digest(rules / (rid + ".yml")) == rule_pins[rid]["sha256"] for rid in IDS),
            "rules changed during benchmark")
    summary = {}
    for engine in ("sql", "rust"):
        samples = [m["wall_seconds"] for m in measurements if m["engine"] == engine]
        tail = samples[1:]
        summary[engine] = {"first_seconds": samples[0], "repeat_n": len(tail),
                           "repeat_median_seconds": statistics.median(tail) if tail else None,
                           "repeat_range_seconds": [min(tail), max(tail)] if tail else None}
    dump(root / "summary.json", summary)


if __name__ == "__main__":
    main()
