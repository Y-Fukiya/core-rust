"""Compare a private resident PostgreSQL, temporary PostgreSQL and core-rs."""

import argparse
import csv
import json
import os
import statistics
import subprocess
import sys
import time
from collections import Counter
from itertools import permutations
from pathlib import Path

import bench as original
from phase_sql import observe_pg


def distribution(values):
    return {"first_seconds": values[0], "repeat_median_seconds": statistics.median(values[1:]),
            "repeat_range_seconds": [min(values[1:]), max(values[1:])], "repeat_n": len(values) - 1}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for argument in ("bench", "out", "pg-root", "bundle"):
        p.add_argument("--" + argument, type=lambda s: Path(s).resolve(), required=True)
    p.add_argument("--reference-label", default="replay",
                   help="Directory label from the completed two-engine comparison")
    a = p.parse_args()
    require, dump, digest = original.require, original.dump, original.digest
    require(a.reference_label.isidentifier(), "reference label must be a simple identifier")
    out = a.out
    require(not any(out.iterdir()), "expected a new empty result directory")
    require(not any(a.pg_root.iterdir()), "expected a new empty private database directory")
    for key in tuple(os.environ):
        if key.startswith(("DATABASE_", "PG", "PYTHON")):
            del os.environ[key]
    for key, folder in (("HOME", a.pg_root / "home"), ("TMPDIR", a.pg_root / "temp")):
        folder.mkdir(mode=0o700)
        os.environ[key] = str(folder)

    src = a.bench / ("cdisc-rules-engine-" + original.SQL_COMMIT)
    require(not (src / ".env").exists(), "unexpected SQL environment overrides")
    reference = a.bench / a.reference_label
    pin = original.read_json(reference / "inputs.json")
    require(pin["sql_commit"] == original.SQL_COMMIT, "SQL source mismatch")
    target = a.bench / "lb-million" / "lb.xpt"
    expected_csv = target.with_name("expected.csv")
    binary = a.bundle / "bin" / "core-rs"
    manifest = original.read_json(a.bundle / "release-manifest.json")
    require(pin["rows"] == 1_000_000 and digest(target) == pin["xpt_sha256"], "XPT mismatch")
    require(digest(expected_csv) == pin["expected_sha256"], "expected CSV mismatch")
    binary_hash = digest(binary)
    require(binary_hash == pin["binary_sha256"], "Rust binary differs from the reference run")
    require(manifest["git"] == {"available": True, "commit": original.RUST_COMMIT, "dirty": False},
            "bundle must come from the pinned clean Rust commit")
    require(manifest["artifacts"] == [{"path": "bin/core-rs", "sha256": binary_hash}],
            "Rust binary does not match its manifest")
    rules = reference / "rules"
    require({f.name for f in rules.iterdir()} == {rid + ".yml" for rid in original.IDS}, "unexpected rules")
    for rid in original.IDS:
        require(digest(rules / (rid + ".yml")) == pin["rules"][rid]["sha256"], "rule mismatch: " + rid)
    dump(out / "inputs.json", pin)
    with expected_csv.open(newline="") as stream:
        expected = Counter(
            original.issue_key(row["rule_id"], row["dataset"], row["row"], row["variables"].split("|"),
                               row["usubjid"], row["seq"])
            for row in csv.DictReader(stream)
        )
    require(len(expected) == sum(expected.values()) == 100, "expected 100 unique findings")

    import pgserver
    import psycopg2

    resident_timings = {}
    observe_pg(resident_timings)
    pgdir = a.pg_root / "resident"
    start = time.perf_counter()
    with pgserver.get_server(pgdir, cleanup_mode="stop") as server:
        resident_timings["setup_total_seconds"] = time.perf_counter() - start
        info = server.get_postmaster_info()
        require(info.socket_dir is not None and info.hostname is None, "resident DB must be Unix-socket only")
        connection = dict(host=str(info.socket_dir), port=info.port, dbname="postgres", user="postgres")

        def pg_settings():
            conn = psycopg2.connect(**connection)
            try:
                with conn.cursor() as cursor:
                    cursor.execute(
                        "SELECT version(), pg_postmaster_start_time()::text, "
                        "current_setting('data_directory'), current_setting('listen_addresses'), "
                        "current_setting('shared_buffers'), current_setting('fsync')"
                    )
                    return cursor.fetchone()
            finally:
                conn.close()

        steady = pg_settings()
        require(Path(steady[2]).resolve() == pgdir and steady[3] == "", "wrong database or network settings")
        dump(out / "resident-startup.json", {
            **resident_timings, "version": steady[0], "shared_buffers": steady[4], "fsync": steady[5],
            "postmaster_pid": info.pid, "postmaster_start_time": steady[1], "unix_socket_only": True,
        })
        print(json.dumps({"resident_setup": resident_timings}), flush=True)
        measurements = []
        orders = permutations(("resident_sql", "temporary_sql", "rust"))
        for pair, order in enumerate(orders):
            for engine in order:
                output = out / f"{pair:02d}-{engine}"
                output.mkdir(mode=0o700)
                env = os.environ.copy()
                phases = output / "phases.json"
                if engine == "rust":
                    command = [binary, "validate", "-s", "sdtmig", "-v", "3.4",
                               "--dataset-path", target, "--local-rules",
                               *(rules / (rid + ".yml") for rid in original.IDS),
                               "--output-format", "json", "-o", output / "rust"]
                else:
                    env["LB_PHASES"] = str(phases)
                    if engine == "resident_sql":
                        env.update(DATABASE_HOST=connection["host"], DATABASE_PORT=str(connection["port"]),
                                   DATABASE_NAME=connection["dbname"], DATABASE_USER=connection["user"])
                    mem = ["-mem"] if engine == "temporary_sql" else []
                    command = [sys.executable, "-B", "-E", "-s", Path(__file__).with_name("phase_sql.py"),
                               src / "core.py", "validate", *mem, "-s", "sdtmig", "-v", "3-4",
                               "-dp", target, "-lr", rules, "-of", "JSON", "-o", output / "sql",
                               "-p", "disabled"]
                load = os.getloadavg()[0]
                with (output / "command.log").open("wb") as stream:
                    start = time.perf_counter()
                    work = subprocess.run(["/usr/bin/time", "-l", *command], cwd=src, env=env,
                                          stdout=stream, stderr=subprocess.STDOUT, timeout=240)
                    wall = time.perf_counter() - start
                require(work.returncode == 0, f"{engine} execution failed: {output / 'command.log'}")
                found = original.inspect("rust" if engine == "rust" else "sql", output, pin["rows"])
                require(found == expected,
                        f"{engine}: missing {sum((expected-found).values())}, extra {sum((found-expected).values())}")
                record = {"pair": pair, "engine": engine, "wall_seconds": wall, "load_1m_before": load,
                          "executed": len(original.IDS), "findings": sum(found.values()), "skips": 0,
                          "expected_match": True}
                if engine != "rust":
                    raw = original.read_json(phases)
                    want = "DatabaseConfigPostgres" if engine == "resident_sql" else "DatabaseConfigPGServer"
                    require(raw.pop("pool_config") == [want], "SQL connected using unexpected database mode")
                    require(len(raw["load_xpt_to_db"]) == 1 and len(raw["validate_rule"]) == 5,
                            "did not time one dataset load and five rule executions")
                    require(("db_start_ready" in raw) == (engine == "temporary_sql"), "unexpected DB startup")
                    record["phases_seconds"] = {key: sum(values) for key, values in raw.items()}
                measurements.append(record)
                dump(out / "measurements.json", measurements)
                print(json.dumps(record), flush=True)
                if engine == "resident_sql":
                    require(pg_settings() == steady, "resident PostgreSQL restarted or settings changed")
    require(not (pgdir / "postmaster.pid").exists(), "test database did not shut down")
    require(digest(target) == pin["xpt_sha256"] and digest(binary) == pin["binary_sha256"],
            "XPT or binary changed")
    require(all(digest(rules / (rid + ".yml")) == pin["rules"][rid]["sha256"] for rid in original.IDS),
            "rule changed")
    summary = {}
    for engine in ("resident_sql", "temporary_sql", "rust"):
        samples = [r for r in measurements if r["engine"] == engine]
        timings = {"wall": distribution([r["wall_seconds"] for r in samples])}
        if engine != "rust":
            for key in samples[0]["phases_seconds"]:
                timings[key] = distribution([r["phases_seconds"][key] for r in samples])
        summary[engine] = timings
    dump(out / "summary.json", summary)


if __name__ == "__main__":
    main()
