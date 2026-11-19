"""Run the AE/DM R regression privately against the CI-built Linux release."""

import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from urllib.request import urlopen

from bundle import PILOT, PUBLIC_INPUTS, read_json, require, sha256, stage


RAW_GITHUB = "https://raw.githubusercontent.com"
SAMPLE_COMMIT = "7ec53ea88dfe1c168f9efbef59233c4426813b89"
RULE_FILES = {
    "CORE-000087": "CG0387,TIG0549.087d5348-f494-4e5b-89fc-bf2c3639def3.yml",
    "CORE-000119": "CG0521,TIG0616.9b0b0834-9c7d-4217-84d3-34ec2ceec994.yml",
    "CORE-000127": "CG0392,TIG0554.25003336-7730-4853-b6d5-d5d54f1922bb.yml",
    "CORE-000185": "CG0123,TIG0380.9d5443f8-7cf2-4bfc-a8b3-546b8d42b16f.yml",
    "CORE-000189": "CG0665,TIG0699.c3dc6682-414f-456f-8b68-3bb18a920da4.yml",
}
BINARY = "bin/core-rs"
MAX_PUBLIC_INPUT_BYTES = 1024 * 1024


def download(path, url, digest):
    with urlopen(url, timeout=30) as response:
        data = response.read(MAX_PUBLIC_INPUT_BYTES + 1)
    require(len(data) <= MAX_PUBLIC_INPUT_BYTES, "public input is too large")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    require(sha256(path) == digest, "public input digest mismatch")


def stage_release(bundle, release_dir, lock_file, ids):
    manifest_name = "release-manifest.json"
    manifest_file = stage(bundle, manifest_name, release_dir / manifest_name)
    manifest = read_json(manifest_file)
    git = manifest["git"]
    target = manifest["target_triple"]
    artifacts = manifest["artifacts"]
    require(
        "-linux-" in target and git["available"] is True and git["dirty"] is False
        and re.fullmatch(r"[0-9a-f]{40}", git["commit"]) is not None
        and len(artifacts) == 1 and artifacts[0]["path"] == BINARY,
        "expected the verified Linux release manifest with one core-rs artifact",
    )
    binary_digest = artifacts[0]["sha256"]
    require(artifacts == [{"path": BINARY, "sha256": binary_digest}]
            and re.fullmatch(r"[0-9a-f]{64}", binary_digest) is not None,
            "invalid artifact declaration")
    stage(bundle, BINARY, release_dir / BINARY, binary_digest)

    # These are fixture expectations for precheck.R, not qualification or SQL evidence.
    receipt = {
        "schema_version": 1,
        "status": "qualified",
        "purpose": "ci-test-fixture-not-release-qualification",
        "pilot_lock_sha256": sha256(lock_file),
        "release_manifest_sha256": sha256(manifest_file),
        "binary_sha256": binary_digest,
        "source_commit": git["commit"],
        "target_triple": target,
        "comparisons": [
            {"case": case, "executed": 5, "findings": int(case != "normal")}
            for case in ["normal", *ids]
        ],
    }
    return bundle / "release-qualification.json", receipt


def stage_public_inputs(bundle, lock):
    source = lock["rule_source"]
    commit = source["commit"]
    require(source["repository"] == "https://github.com/cdisc-org/core-contributor"
            and re.fullmatch(r"[0-9a-f]{40}", commit) is not None, "invalid rule source")
    for rule in lock["rules"]:
        rule_id = rule["id"]
        download(bundle / "rules" / f"{rule_id}.yml",
                 f"{RAW_GITHUB}/cdisc-org/core-contributor/{commit}/sdtm_rules/"
                 f"{rule_id}/{RULE_FILES[rule_id]}", rule["sha256"])
    for name, digest in PUBLIC_INPUTS.items():
        download(bundle / "sample-data" / name,
                 f"{RAW_GITHUB}/pharmaverse/pharmaversesdtm/{SAMPLE_COMMIT}/data/{name}", digest)


def run_r(script, args, root, r_tmp):
    # R failures can expose paths and row-level details. Only report the stage.
    subprocess.run(
        ["Rscript", "--vanilla", script, *args], cwd=root,
        env={**os.environ, "TMPDIR": str(r_tmp)},
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-dir", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    args = parser.parse_args()
    phase = "temporary workspace setup"
    try:
        with tempfile.TemporaryDirectory(prefix="ae-dm-r-ci-", dir=args.work_dir.resolve()) as directory:
            root = Path(directory)
            bundle = root / "bundle"
            r_tmp = root / "r-tmp"
            r_tmp.mkdir(mode=0o700)

            phase = "pilot lock and release verification"
            lock_file = stage(bundle, "pilot-lock.json", PILOT / "pilot-lock.json")
            lock = read_json(lock_file)
            ids = [rule["id"] for rule in lock["rules"]]
            require(len(ids) == len(RULE_FILES) and set(ids) == set(RULE_FILES)
                    and all(rule["control"]["folder"] == rule["id"][-3:] for rule in lock["rules"]),
                    "unexpected pilot rules or control folders")
            receipt_file, receipt = stage_release(bundle, args.release_dir, lock_file, ids)
            receipt_file.write_text(json.dumps(receipt), encoding="utf-8")

            phase = "pinned public inputs"
            stage_public_inputs(bundle, lock)
            for script in ("example.R", "precheck.R", "smoke.R"):
                stage(bundle, script, PILOT / script)

            phase = "R example (normal, five controls, missing column, repair)"
            project = root / "example"
            run_r(bundle / "example.R", [bundle, project], root, r_tmp)

            phase = "smoke input staging"
            normal = project / "normal"
            shutil.copytree(bundle / "rules", normal / "rules")
            mutants = root / "mutants"
            mutants.mkdir()
            for index, rule in enumerate(lock["rules"], start=1):
                (project / f"abnormal-{index}").rename(mutants / rule["control"]["folder"])

            phase = "R smoke (five findings, unavailable paths, runtime snapshots)"
            run_r(bundle / "smoke.R", [bundle, bundle, receipt_file, normal, mutants], root, r_tmp)
            phase = "temporary workspace cleanup"
    except subprocess.CalledProcessError as error:
        parser.exit(1, f"AE/DM R CI failed: {phase} (Rscript exit {error.returncode}).\n")
    except (OSError, ValueError, KeyError, IndexError, TypeError):
        parser.exit(1, f"AE/DM R CI failed: {phase}.\n")
    print("AE/DM R CI passed: example and smoke checks (CI fixture only; no release qualification).")


if __name__ == "__main__":
    main()
