"""Copy the pinned curated subset and apply its reviewed oracle replacements."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

from open_rules_pilot_check import source_hash


FIXTURES = Path(__file__).resolve().parents[1] / "tests/open_rules"
REVIEWED_CASES = {
    "CORE-000677": {"negative/01", "negative/02", "positive/02"},
    "CORE-000206": {"negative/02"},
}


def prepare(upstream_root: Path, subset_root: Path) -> None:
    upstream_root = upstream_root.resolve(strict=True)
    if subset_root.exists() or subset_root.is_symlink():
        raise ValueError(f"subset destination already exists: {subset_root}")
    subset_root = subset_root.resolve()
    if any(parent.exists() and parent.samefile(upstream_root) for parent in subset_root.parents):
        raise ValueError(f"subset destination is inside upstream: {subset_root}")

    rule_ids = []
    for line in (FIXTURES / "curated-upstream-rules.txt").read_text(encoding="utf-8").splitlines():
        rule_id = line.strip()
        if not rule_id or rule_id.startswith("#"):
            continue
        if not re.fullmatch(r"CORE-[0-9]{6}", rule_id) or rule_id in rule_ids:
            raise ValueError(f"invalid or duplicate curated rule ID: {rule_id}")
        rule_ids.append(rule_id)
    for reviewed_rule in REVIEWED_CASES:
        if reviewed_rule not in rule_ids:
            raise ValueError(f"reviewed rule is not in the curated subset: {reviewed_rule}")

    pins = [line.removeprefix("sha=") for line in
            (FIXTURES / "upstream.lock").read_text(encoding="utf-8").splitlines()
            if line.startswith("sha=")]
    if len(pins) != 1 or not re.fullmatch(r"[0-9a-f]{40}", pins[0]):
        raise ValueError("upstream.lock must have exactly one pinned commit SHA")

    published = upstream_root / "Published"
    if published.is_symlink():
        raise ValueError(f"symlink in upstream tree: {published}")
    for rule_id in rule_ids:
        rule_dir = published / rule_id
        if not rule_dir.is_dir():
            raise ValueError(f"missing curated rule directory: {rule_dir}")
        if rule_dir.is_symlink() or any(path.is_symlink() for path in rule_dir.rglob("*")):
            raise ValueError(f"symlink in upstream rule: {rule_dir}")
    overlays = {}
    for reviewed_rule, cases in REVIEWED_CASES.items():
        reviewed = FIXTURES / "curated-reviewed" / reviewed_rule
        manifest_path = reviewed / "manifest.json"
        if manifest_path.resolve() != manifest_path:
            raise ValueError(f"symlink in reviewed tree: {manifest_path}")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if (not isinstance(manifest, dict) or type(manifest.get("schema_version")) is not int
                or manifest["schema_version"] != 1 or manifest.get("rule_id") != reviewed_rule):
            raise ValueError(f"invalid reviewed manifest: {manifest_path}")
        if manifest.get("upstream_sha") != pins[0]:
            raise ValueError(f"reviewed manifest upstream SHA does not match upstream.lock: {reviewed_rule}")
        replacements = manifest.get("replacements")
        if not isinstance(replacements, dict) or replacements.keys() != cases:
            raise ValueError(f"reviewed case keys must be exactly: {', '.join(sorted(cases))} ({reviewed_rule})")
        if source_hash(published / reviewed_rule) != manifest.get("source_sha256"):
            raise ValueError(f"source changed or missing: {reviewed_rule}")

        for case, digest in replacements.items():
            target = Path("Published") / reviewed_rule / case / "results/results.csv"
            if not (upstream_root / target).is_file():
                raise ValueError(f"missing original oracle CSV: {target}")
            path = reviewed / case / "results.csv"
            if path.resolve() != path:
                raise ValueError(f"symlink in reviewed tree: {path}")
            data = path.read_bytes()
            if hashlib.sha256(data).hexdigest() != digest:
                raise ValueError(f"replacement hash mismatch: {path}")
            overlays[target] = data

    subset_root.mkdir()
    for rule_id in rule_ids:
        shutil.copytree(published / rule_id, subset_root / "Published" / rule_id)
    for target, data in overlays.items():
        (subset_root / target).write_bytes(data)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream-root", type=Path, required=True)
    parser.add_argument("--subset-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        prepare(args.upstream_root, args.subset_root)
    except (OSError, ValueError, RuntimeError) as exc:
        parser.exit(1, f"{parser.prog}: {exc}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
