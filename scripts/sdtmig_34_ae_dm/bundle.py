"""Package the existing macOS arm64 AE/DM qualification for a local pilot."""

import argparse
import gzip
import hashlib
import json
import re
import shutil
import tarfile
import tempfile
from pathlib import Path


PILOT = Path(__file__).resolve().parent
TARGET = "aarch64-apple-darwin"
BUNDLE_NAME = "core-rs-ae-dm-pilot-macos-arm64"
PUBLIC_INPUTS = {
    "ae.rda": "dae58611f970a2bed9071999da3c390548c66d58a1e5430472a935aadc887c17",
    "dm.rda": "36364acc1d1e16fc72d6b28d04a9677df2375f543a46abe75d609de9835176fb",
}
NOTICES = {
    "core-contributor-LICENSE.txt": "25787bea0c90e13504ff48bf2156844722f4f2e08d385aaba4069e17b1821610",
    "pharmaversesdtm-LICENSE.md": "4bb8faa5dc73b0450322ff73edd9162062889bce6c1a51b698707b2ae04de976",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stage(root, destination, source, digest=None):
    path = root / destination
    path.parent.mkdir(parents=True, exist_ok=True)
    require(source.is_file(), f"missing input: {source.name}")
    shutil.copyfile(source, path)
    if digest is not None:
        require(sha256(path) == digest, f"digest mismatch: {destination}")
    path.chmod(0o755 if destination == "bin/core-rs" else 0o644)
    return path


def read_json(path):
    return json.loads(path.read_bytes())


def assemble(args, root):
    proof = read_json(stage(root, "release-qualification.json", PILOT / "release-qualification.json"))
    require(proof["schema_version"] == 1 and proof["status"] == "qualified"
            and proof["target_triple"] == TARGET, "expected a qualified macOS arm64 receipt")
    lock = read_json(stage(root, "pilot-lock.json", PILOT / "pilot-lock.json", proof["pilot_lock_sha256"]))
    ids = [rule["id"] for rule in lock["rules"]]
    require(lock["schema_version"] == 1 and (lock["standard"], lock["standard_version"]) == ("sdtmig", "3.4")
            and len(ids) == len(set(ids)) == 5 and all(re.fullmatch(r"CORE-[0-9]{6}", rule) for rule in ids),
            "expected five distinct SDTMIG 3.4 rule IDs")
    require(
        [(c["case"], c["executed"], c["findings"]) for c in proof["comparisons"]]
        == [(case, 5, int(case != "normal")) for case in ["normal", *ids]],
        "receipt does not cover all six public controls",
    )
    manifest = read_json(stage(root, "release-manifest.json", args.release_dir / "release-manifest.json",
                               proof["release_manifest_sha256"]))
    binary = "bin/core-rs"
    require(
        manifest["target_triple"] == TARGET and manifest["git"]["commit"] == proof["source_commit"]
        and manifest["git"]["available"] is True and manifest["git"]["dirty"] is False
        and manifest["artifacts"] == [{"path": binary, "sha256": proof["binary_sha256"]}],
        "release manifest does not match the qualification receipt",
    )
    stage(root, binary, args.release_dir / binary, proof["binary_sha256"])
    for rule in lock["rules"]:
        name = rule["id"] + ".yml"
        stage(root, "rules/" + name, args.rules_dir / name, rule["sha256"])
    for name, digest in PUBLIC_INPUTS.items():
        stage(root, "sample-data/" + name, args.sample_dir / name, digest)
    for name, digest in NOTICES.items():
        stage(root, "licenses/" + name, args.notices_dir / name, digest)
    stage(root, "licenses/core-rust-LICENSE.txt", PILOT.parents[1] / "LICENSE")
    for name in ("README.md", "precheck.R", "example.R"):
        stage(root, name, PILOT / name)


def portable_entry(info):
    info.uid = info.gid = info.mtime = 0
    info.uname = info.gname = ""
    info.mode = 0o755 if info.isdir() or info.name.endswith("/bin/core-rs") else 0o644
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("release-dir", "rules-dir", "sample-dir", "notices-dir", "out"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix="ae-dm-bundle-") as directory:
            root = Path(directory) / BUNDLE_NAME
            assemble(args, root)
            with args.out.open("xb") as stream:
                with gzip.GzipFile(filename="", fileobj=stream, mode="wb", mtime=0) as compressed:
                    with tarfile.open(fileobj=compressed, mode="w|") as archive:
                        archive.add(root, arcname=BUNDLE_NAME, filter=portable_entry)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"error: {error}\n")
    print(f"{sha256(args.out)}  {args.out.name}")


if __name__ == "__main__":
    main()
