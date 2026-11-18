"""Qualify one release build on the six locked public AE/DM controls."""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

import pyreadstat


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require(equal, description):
    if not equal:
        raise ValueError(description)


def read_json(path):
    return json.loads(path.read_bytes())


def run(command, log, *, cwd=None):
    with log.open("wb") as stream:
        subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, check=True)


def snapshot(source, destination, expected=None):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy(source, destination)
    if expected is not None:
        require(sha256(destination) == expected, "digest mismatch: " + source.name)
    return destination


def compare(sql, rust, lock, datasets, mutant):
    rules = lock["rules"]
    bad_id = mutant["id"] if mutant else None
    details = sql["Conformance_Details"]
    require(
        (details["CORE_Engine_Version"], details["Standard"], details["Version"])
        == (lock["sql"]["version"], "SDTMIG", "V3.4"),
        "unexpected SQL engine or standard",
    )
    require(
        sorted((d["filename"], d["length"]) for d in sql["Dataset_Details"])
        == sorted((name, d["rows"]) for name, d in lock["datasets"].items()),
        "unexpected SQL input domains or row counts",
    )
    meta = rust["metadata"]
    require(
        (meta["engine"], meta["standard"], meta["standard_version"], meta["dataset_count"])
        == ("core-rs", lock["standard"], lock["standard_version"], len(datasets)),
        "unexpected Rust engine, standard, or input count",
    )
    require(
        sorted((r["core_id"], r["status"], r["skip_reason"]) for r in sql["Rules_Report"])
        == sorted((r["id"], "ISSUE REPORTED" if r["id"] == bad_id else "SUCCESS", "") for r in rules),
        "SQL rule missing, skipped, or has an unexpected result",
    )
    require(
        sorted(
            (r["rule_id"], r["dataset"], r["domain"], r["execution_status"], r["error_count"], len(r["errors"]))
            for r in rust["results"]
        )
        == sorted(
            (r["id"], r["domain"], r["domain"], "failed", 1, 1)
            if r["id"] == bad_id else (r["id"], r["domain"], r["domain"], "passed", 0, 0)
            for r in rules
        ),
        "Rust rule missing, skipped, or has an unexpected result",
    )
    rust_issues = [issue for result in rust["results"] for issue in result["errors"]]
    if mutant is None:
        require(not sql["Issue_Summary"] and not sql["Issue_Details"] and not rust_issues, "normal has findings")
        return {"case": "normal", "executed": len(rules), "findings": 0}

    case = mutant["control"]
    domain, row, seq = mutant["domain"], case["row"], case["seq"]
    require(
        [(s["core_id"], s["dataset"], s["issues"]) for s in sql["Issue_Summary"]] == [(bad_id, domain, 1)],
        "SQL finding summary does not match",
    )
    require(len(sql["Issue_Details"]) == len(rust_issues) == 1, "expected one detailed finding per engine")
    df, _ = pyreadstat.read_xport(str(datasets[domain]), usecols=["USUBJID", "AESEQ"] if seq else ["USUBJID"])
    subject = df.iloc[row - 1]["USUBJID"]
    require(isinstance(subject, str) and bool(subject), "source subject is missing")
    if seq is not None:
        require(df.iloc[row - 1]["AESEQ"] == seq, "source sequence changed")
    s, r = sql["Issue_Details"][0], rust_issues[0]
    variables = sorted(mutant["variables"])
    expected = (bad_id, domain, row, variables, subject)
    require((s["core_id"], s["dataset"], s["row"], sorted(s["variables"]), s["USUBJID"]) == expected,
            "SQL finding location does not match")
    require((r["rule_id"], r["dataset"], r["row"], sorted(r["variables"]), r["usubjid"]) == expected
            and r["domain"] == domain, "Rust finding location does not match")
    expected_seq = ("", None) if seq is None else (seq, str(seq))
    require((s["SEQ"], r["seq"]) == expected_seq, "finding sequence differs")
    return {"case": bad_id, "executed": len(rules), "findings": 1, "domain": domain, "record": row,
            "variables": variables, "seq": seq, "subject_matches_input": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("lock", "bundle", "source-root", "xtask", "sql-archive", "normal", "mutants", "out"):
        parser.add_argument("--" + arg, type=lambda p: Path(p).resolve(), required=True)
    args = parser.parse_args()
    args.out.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix=".inputs-", dir=args.out) as scratch:
        receipt = qualify(args, Path(scratch))
    (args.out / "qualification.json").write_text(json.dumps(receipt, indent=2) + "\n")


def qualify(args, scratch):
    lock_bytes = args.lock.read_bytes()
    lock = json.loads(lock_bytes)
    lock_hash = hashlib.sha256(lock_bytes).hexdigest()
    require(lock["schema_version"] == 1, "unsupported pilot lock version")
    sql_archive = snapshot(args.sql_archive, scratch / "sql.tar.gz", lock["sql"]["archive_sha256"])
    with tarfile.open(sql_archive, "r:gz") as archive:
        archive.extractall(scratch, filter="data")
    sql_root = scratch / ("cdisc-rules-engine-" + lock["sql"]["commit"])
    rule_dir = scratch / "rules"
    stems = {rule["id"] + ".yml": rule["sha256"] for rule in lock["rules"]}
    for file, digest in stems.items():
        snapshot(args.normal / "rules" / file, rule_dir / file, digest)
    bundle = scratch / "bundle"
    manifest_file = snapshot(args.bundle / "release-manifest.json", bundle / "release-manifest.json")
    manifest_hash = sha256(manifest_file)
    manifest = read_json(manifest_file)
    binary = snapshot(args.bundle / "bin" / "core-rs", bundle / "bin" / "core-rs")
    binary_hash = sha256(binary)
    require(manifest["artifacts"] == [{"path": "bin/core-rs", "sha256": binary_hash}], "unexpected release artifact")
    source_root = scratch / "source"
    snapshot(args.source_root / "Cargo.lock", source_root / "Cargo.lock")
    host = subprocess.check_output(["rustc", "-vV"], text=True).split("host: ", 1)[1].splitlines()[0]
    run(
        [args.xtask, "release-verify", "--manifest", manifest_file, "--source-root", source_root,
         "--artifact-root", bundle, "--target-triple", host, "--require-clean-git", "--require-artifact",
         "--require-cargo-lock", "--require-source-date-epoch"],
        args.out / "release-verify.log",
    )

    comparisons = []
    for mutant in [None, *lock["rules"]]:
        case = mutant["control"] if mutant else None
        data_dir = args.mutants / case["folder"] if case else args.normal
        output = args.out / (case["folder"] if case else "normal")
        output.mkdir(mode=0o700)
        datasets = {}
        for domain, dataset in lock["datasets"].items():
            digest = case["sha256"] if mutant and mutant["domain"] == domain else dataset["sha256"]
            name = dataset["name"]
            datasets[domain] = snapshot(data_dir / name, scratch / output.name / name, digest)

        run(
            [sys.executable, "-B", "-E", "-s", "core.py", "validate", "-mem", "-s", "sdtmig", "-v", "3-4",
             "-dp", datasets["AE"], "-dp", datasets["DM"], "-lr", rule_dir,
             "-of", "JSON", "-o", output / "sql", "-p", "disabled"],
            output / "sql.log", cwd=sql_root,
        )
        run(
            [binary, "validate", "-s", lock["standard"], "-v", lock["standard_version"],
             "--dataset-path", *datasets.values(), "--local-rules", *(rule_dir / file for file in stems),
             "--output-format", "json", "-o", output / "rust"],
            output / "rust.log",
        )
        comparison = compare(read_json(output / "sql.json"), read_json(output / "rust" / "report.json"),
                             lock, datasets, mutant)
        comparisons.append(comparison)
        print(comparison["case"] + ": structural agreement", flush=True)

    require(sha256(binary) == binary_hash, "binary changed during qualification")
    require(sha256(manifest_file) == manifest_hash, "manifest changed during qualification")
    for file, digest in stems.items():
        require(sha256(rule_dir / file) == digest, "rule changed during qualification: " + file)
    return {
        "schema_version": 1, "status": "qualified", "pilot_lock_sha256": lock_hash,
        "release_manifest_sha256": manifest_hash, "binary_sha256": binary_hash,
        "source_commit": manifest["git"]["commit"], "target_triple": host, "comparisons": comparisons,
    }


if __name__ == "__main__":
    main()
