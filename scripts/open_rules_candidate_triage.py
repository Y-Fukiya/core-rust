#!/usr/bin/env python3
"""Read-only diagnostic classification of paired Open Rules scoreboards.

Families describe YAML metadata, never root causes. Differences are scorer output,
not raw reports, even in strict mode. No engine or scorer code is imported.
"""

from __future__ import annotations

import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

import yaml


KEY_FIELDS = ("scope", "rule_id", "case_kind", "case_id")
BUCKETS = (
    "supported_match", "supported_mismatch", "deferred_oracle_gap_mismatch",
    "deferred_oracle_gap_skipped", "skipped_unsupported", "mixed_skipped_and_issues",
    "no_official_oracle", "harness_error",
)
MISMATCHES = {"supported_mismatch", "deferred_oracle_gap_mismatch"}
VARIABLE_TYPES = {
    "Variable Metadata Check", "Variable Metadata Check against Library Metadata",
    "Variable Metadata Check against Define XML", "Value Check with Variable Metadata",
}
EMPTY_OPERATORS = {"empty", "non_empty", "exists", "not_exists", "empty_within_except_last_row"}
DATE_OPERATORS = {
    "invalid_date", "invalid_duration", "is_complete_date", "is_incomplete_date",
    "date_equal_to", "date_not_equal_to", "date_less_than", "date_greater_than",
    "date_less_than_or_equal_to", "date_greater_than_or_equal_to",
}
COMPARATORS = {
    "equal_to", "not_equal_to", "equal_to_case_insensitive", "not_equal_to_case_insensitive",
    "greater_than", "less_than", "greater_than_or_equal_to", "less_than_or_equal_to",
    "is_contained_by", "is_not_contained_by", "is_contained_by_case_insensitive",
    "contains", "does_not_contain", "contains_case_insensitive",
    "does_not_contain_case_insensitive", "not_contains_all", "shares_no_elements_with",
    "longer_than", "has_not_equal_length",
}
RELATIONSHIP_OPERATORS = {
    "is_not_unique_set", "is_unique_set", "is_not_unique_relationship",
    "is_inconsistent_across_dataset", "present_on_multiple_rows_within",
    "not_present_on_multiple_rows_within", "does_not_have_next_corresponding_record",
    "target_is_not_sorted_by", "is_not_ordered_subset_of", "inconsistent_enumerated_columns",
}
STRING_OPERATORS = {
    "matches_regex", "not_matches_regex", "matches_regex_case_insensitive",
    "prefix_matches_regex", "not_prefix_matches_regex", "suffix_matches_regex",
    "not_suffix_matches_regex", "not_prefix_matches_regex_case_insensitive",
    "prefix_is_not_contained_by", "prefix_is_not_contained_by_case_insensitive",
    "suffix_is_not_contained_by_case_insensitive", "prefix_not_equal_to",
    "does_not_equal_string_part", "starts_with", "ends_with",
}
PRECEDENCE = (
    "domain_presence", "variable_metadata", "group_sensitivity", "dataset_sensitivity",
    "record_data_date", "operation_distinct", "operation_other",
    "record_data_relationship", "record_data_string", "record_data_comparator",
    "record_data_empty_existence", "unclassified",
)


class InputError(ValueError):
    """Input evidence cannot be safely compared."""


def _unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InputError(f"duplicate key: {key}")
        result[key] = value
    return result


class UniqueSafeLoader(yaml.SafeLoader):
    pass


def _yaml_mapping(loader, node):
    loader.flatten_mapping(node)
    pairs = []
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=True)
        if not isinstance(key, str):
            raise InputError("YAML mapping keys must be strings")
        pairs.append((key, loader.construct_object(value_node, deep=True)))
    return _unique_pairs(pairs)


UniqueSafeLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _yaml_mapping)
UniqueSafeLoader.add_constructor("tag:yaml.org,2002:timestamp", lambda loader, node: node.value)


def _json(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True, allow_nan=False)


def _integer(value, label):
    if type(value) is not int or value < 0:
        raise InputError(f"{label} must be a nonnegative integer")
    return value


def _key(case):
    if not isinstance(case, dict):
        raise InputError("case must be an object")
    parts = tuple(case.get(field) for field in KEY_FIELDS)
    if any(not isinstance(p, str) or not p or p in {".", ".."}
           or any(c in p for c in "/\\\0") for p in parts):
        raise InputError("case key must contain four nonempty path-component strings")
    if parts[2] not in {"positive", "negative"}:
        raise InputError(f"invalid case_kind: {parts[2]}")
    return parts


def _validate_summary(summary, cases, label):
    if not isinstance(summary, dict):
        raise InputError(f"{label}: missing summary")
    counts = Counter(c["bucket"] for c in cases)
    for field, actual in [("total_cases", len(cases)), *[(b, counts[b]) for b in BUCKETS]]:
        if _integer(summary.get(field), f"{label}.{field}") != actual:
            raise InputError(f"{label}.{field}: declared count differs from actual {actual}")


def _validate_case(case):
    key = _key(case)
    if case.get("bucket") not in BUCKETS:
        raise InputError(f"{key}: unknown bucket")
    for field in ("official_issue_count", "candidate_issue_count", "missing_count", "extra_count"):
        if case.get(field) is not None:
            _integer(case[field], f"{key}.{field}")
        elif case["bucket"] in MISMATCHES | {"supported_match"}:
            raise InputError(f"{key}: missing {field}")
    for field in ("missing", "extra"):
        rows = case.get(field, [])
        if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
            raise InputError(f"{key}.{field}: expected structural row objects")
        count = case.get(f"{field}_count")
        if rows and (count is None or len(rows) > count):
            raise InputError(f"{key}.{field}: sample exceeds declared count")
        for row in rows:
            if any(not isinstance(row.get(f), str)
                   for f in ("rule_id", "dataset", "domain", "row", "usubjid", "seq")):
                raise InputError(f"{key}.{field}: invalid structural locator")
            if not isinstance(row.get("variables"), list) or not all(
                isinstance(v, str) for v in row["variables"]
            ):
                raise InputError(f"{key}.{field}: invalid variables")
    counts = [case.get(f) for f in (
        "official_issue_count", "candidate_issue_count", "missing_count", "extra_count"
    )]
    if all(n is not None for n in counts):
        official, candidate, missing, extra = counts
        if missing > official or extra > candidate or official - missing != candidate - extra:
            raise InputError(f"{key}: inconsistent structural counts")
        if case["bucket"] == "supported_match" and (missing or extra):
            raise InputError(f"{key}: match has structural differences")
    return key


def read_scoreboard(path):
    def reject_constant(value):
        raise InputError(f"invalid JSON constant: {value}")

    board = json.loads(path.read_text(), object_pairs_hook=_unique_pairs, parse_constant=reject_constant)
    if not isinstance(board, dict) or not isinstance(board.get("cases"), list) or not board["cases"]:
        raise InputError(f"{path}: expected nonempty cases array")
    upstream = board.get("upstream")
    if not isinstance(upstream, dict) or not isinstance(upstream.get("repo"), str) or not upstream["repo"]:
        raise InputError(f"{path}: missing upstream repo")
    for field in ("observed_sha", "expected_sha"):
        if not isinstance(upstream.get(field), str) or not re.fullmatch(r"[0-9a-f]{40}", upstream[field]):
            raise InputError(f"{path}: invalid upstream {field}")
    indexed = {}
    for case in board["cases"]:
        key = _validate_case(case)
        if key in indexed:
            raise InputError(f"{path}: duplicate case key {key}")
        indexed[key] = case
    _validate_summary(board.get("summary"), board["cases"], str(path))
    for group_field, key_field in (("by_scope", "scope"), ("by_case_kind", "case_kind")):
        if group_field not in board:
            continue
        groups = board[group_field]
        if not isinstance(groups, list) or not all(isinstance(g, dict) for g in groups):
            raise InputError(f"{path}: invalid {group_field}")
        names = [g.get("name") for g in groups]
        expected = {c[key_field] for c in indexed.values()}
        if not all(isinstance(n, str) for n in names) or len(names) != len(set(names)) or set(names) != expected:
            raise InputError(f"{path}: invalid {group_field} names")
        for group in groups:
            _validate_summary(group.get("summary"), [c for c in indexed.values() if c[key_field] == group["name"]], group_field)
    return board, indexed


def _walk(value, path):
    yield path, value
    if isinstance(value, dict):
        for key in sorted(value):
            yield from _walk(value[key], f"{path}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            yield from _walk(item, f"{path}[{index}]")


def classify(metadata):
    rule_type, sensitivity = metadata.get("Rule Type"), metadata.get("Sensitivity")
    nodes = list(_walk(metadata.get("Check"), "Check"))
    operations = list(_walk(metadata.get("Operations"), "Operations"))
    operators = sorted({v for p, v in nodes if p.endswith(".operator") and isinstance(v, str)})
    operation_names = sorted({v for p, v in operations if p.endswith(".operator") and isinstance(v, str)})
    op = set(operators)
    record = rule_type == "Record Data" and sensitivity == "Record"
    candidates = {
        "domain_presence": rule_type == "Domain Presence Check",
        "variable_metadata": rule_type in VARIABLE_TYPES,
        "group_sensitivity": rule_type == "Record Data" and sensitivity == "Group",
        "dataset_sensitivity": rule_type == "Record Data" and sensitivity == "Dataset",
        "record_data_date": record and bool(op & DATE_OPERATORS or set(operation_names) & {"min_date", "max_date", "dy", "valid_codelist_dates"}),
        "operation_distinct": record and "distinct" in operation_names,
        "operation_other": record and bool(operation_names),
        "record_data_relationship": record and bool(op & RELATIONSHIP_OPERATORS),
        "record_data_string": record and bool(op & STRING_OPERATORS),
        "record_data_comparator": record and bool(op & COMPARATORS),
        "record_data_empty_existence": record and bool(op) and op <= EMPTY_OPERATORS,
    }
    matched = [family for family in PRECEDENCE if candidates.get(family)]
    family = matched[0] if matched else "unclassified"
    outcome = metadata.get("Outcome") or {}
    traits = {
        "rule_type": rule_type, "sensitivity": sensitivity,
        "operators": operators, "operations": operation_names,
        "authorities": metadata.get("Authorities"), "scope": metadata.get("Scope"),
        "output_variables": outcome.get("Output Variables"),
        "operator_paths": {p: v for p, v in nodes + operations if p.endswith(".operator")},
        "domain_references": {p: v for p, v in nodes + operations if p.endswith(".domain") or
                              (p.endswith((".name", ".value")) and isinstance(v, str) and "." in v)},
    }
    return {
        "primary_family": family, "matched_families": matched,
        "classification_reason": f"First matching family in declared precedence: {family}" if matched else "No recognized metadata family",
        "metadata_paths": [p for p in ("Rule Type", "Sensitivity", "Check", "Operations", "Scope", "Authorities", "Outcome.Output Variables")
                           if (p in metadata or (p == "Outcome.Output Variables" and "Output Variables" in outcome))],
        "traits": traits,
    }


def _rule(root, scope, rule_id, tracked_blobs):
    path = root / scope / rule_id / "rule.yml"
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(root):
        raise InputError(f"rule path escapes rules root: {path}")
    content = resolved.read_bytes()
    relative_path = path.relative_to(root).as_posix()
    blob_id = hashlib.sha1(b"blob " + str(len(content)).encode("ascii") + b"\0" + content).hexdigest()
    if tracked_blobs.get(relative_path) != blob_id:
        raise InputError(f"{path}: rule bytes differ from tracked HEAD blob or rule is untracked")
    metadata = yaml.load(content, Loader=UniqueSafeLoader)
    if not isinstance(metadata, dict):
        raise InputError(f"{path}: rule YAML must be a mapping")
    core = metadata.get("Core")
    if not isinstance(core, dict) or core.get("Id") != rule_id:
        raise InputError(f"{path}: Core.Id does not match case rule_id")
    for field in ("Rule Type", "Sensitivity"):
        if metadata.get(field) is not None and not isinstance(metadata[field], str):
            raise InputError(f"{path}: {field} must be a string")
    for field in ("Outcome", "Scope"):
        if metadata.get(field) is not None and not isinstance(metadata[field], dict):
            raise InputError(f"{path}: {field} must be a mapping")
    if metadata.get("Operations") is not None and (
        not isinstance(metadata["Operations"], list) or
        not all(isinstance(o, dict) and isinstance(o.get("operator"), str) for o in metadata["Operations"])
    ):
        raise InputError(f"{path}: Operations must contain operator mappings")
    result = classify(metadata)
    result.update(rule_yaml_path=relative_path, rule_yaml_sha256=hashlib.sha256(content).hexdigest(), rule_git_blob=blob_id)
    return result


def _suspicions(case, traits):
    official, candidate = case.get("official_issue_count"), case.get("candidate_issue_count")
    if (case["bucket"] in MISMATCHES and traits["sensitivity"] in {"Dataset", "Group"}
            and official is not None and candidate is not None and min(official, candidate) > 0
            and official != candidate):
        return [{"hypothesis": "reporting_granularity", "status": "suspected",
                 "evidence": {"sensitivity": traits["sensitivity"], "official_issue_count": official,
                              "candidate_issue_count": candidate},
                 "limitation": "Sensitivity and unequal nonzero scoreboard counts only; raw reports and cause are unverified."}]
    return []


def build_report(default_path, strict_path, rules_root):
    default, default_cases = read_scoreboard(default_path)
    strict, strict_cases = read_scoreboard(strict_path)
    for field in ("repo", "observed_sha", "expected_sha"):
        if default["upstream"][field] != strict["upstream"][field]:
            raise InputError(f"paired upstream {field} differs")
    if default_cases.keys() != strict_cases.keys():
        raise InputError("paired case key sets differ")
    root = rules_root.resolve(strict=True)
    if not (root / ".git").exists():
        raise InputError("rules root must be a git checkout to verify observed_sha")
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    if head != default["upstream"]["observed_sha"]:
        raise InputError("rules git HEAD differs from observed_sha")
    tree = subprocess.run(["git", "-C", str(root), "ls-tree", "-rz", "HEAD"], capture_output=True, check=True).stdout
    tracked_blobs = {}
    for entry in tree.split(b"\0"):
        if not entry:
            continue
        header, name = entry.split(b"\t", 1)
        mode, kind, blob_id = header.split()
        if kind == b"blob" and mode in {b"100644", b"100755"}:
            tracked_blobs[name.decode("utf-8")] = blob_id.decode("ascii")
    rules = {}
    cases = []
    for key in sorted(default_cases):
        scope, rule_id, _, _ = key
        if (scope, rule_id) not in rules:
            rules[scope, rule_id] = _rule(root, scope, rule_id, tracked_blobs)
        classification = rules[scope, rule_id]
        before, after = default_cases[key], strict_cases[key]
        cases.append({
            "key": dict(zip(KEY_FIELDS, key)), **classification,
            "default_bucket": before["bucket"], "strict_bucket": after["bucket"],
            "default_mismatch": before["bucket"] in MISMATCHES,
            "strict_only_mismatch": before["bucket"] == "supported_match" and after["bucket"] in MISMATCHES,
            "bucket_changed": before["bucket"] != after["bucket"],
            "scoreboard_evidence": {"default": before, "strict": after},
            "suspected_causes": _suspicions(before, classification["traits"]),
        })
    mismatches = [c for c in cases if c["default_mismatch"]]
    family_counts = {}
    for family in sorted({c["primary_family"] for c in mismatches}):
        selected = [c for c in mismatches if c["primary_family"] == family]
        family_counts[family] = {"total": len(selected), **{b: sum(c["default_bucket"] == b for c in selected) for b in sorted(MISMATCHES)}}
    reconciliation = Counter()
    for case in mismatches:
        traits = case["traits"]
        if traits["rule_type"] == "Record Data":
            reconciliation[f"Record Data / {traits['sensitivity']}"] += 1
        else:
            reconciliation[case["primary_family"]] += 1
    return {
        "schema_version": 1, "purpose": "diagnostic_metadata_classification_only",
        "evidence_kind": "scoreboard_structural_differences_not_raw",
        "limitations": ["Families describe rule metadata, not confirmed root causes.",
                        "Strict scoring is not raw evidence; both inputs may already be normalized.",
                        "Missing/extra lists may be bounded samples; retain their separate total counts and multiplicity.",
                        "Historical report references are preserved verbatim, not opened or remapped.",
                        "Every classified YAML matches its tracked HEAD blob; other checkout artifacts are not validated.",
                        "No official rerun, adoption decision, engine execution or re-scoring is performed."],
        "upstream": default["upstream"], "strict_upstream": strict["upstream"],
        "rules_git_head": head, "family_precedence": list(PRECEDENCE),
        "inputs": {"default_scoreboard": str(default_path.resolve()), "strict_scoreboard": str(strict_path.resolve()), "rules_root": str(root)},
        "summary": {
            "total_cases": len(cases), "default_mismatches": len(mismatches),
            "default_mismatch_rules": len({(c["key"]["scope"], c["key"]["rule_id"]) for c in mismatches}),
            "default_buckets": dict(sorted(Counter(c["default_bucket"] for c in cases).items())),
            "strict_buckets": dict(sorted(Counter(c["strict_bucket"] for c in cases).items())),
            "default_mismatch_families": family_counts,
            "all_case_families": dict(sorted(Counter(c["primary_family"] for c in cases).items())),
            "default_mismatch_metadata_reconciliation": dict(sorted(reconciliation.items())),
            "strict_only_mismatches": sum(c["strict_only_mismatch"] for c in cases),
            "bucket_transitions": dict(sorted(Counter(f"{c['default_bucket']} -> {c['strict_bucket']}" for c in cases).items())),
        },
        "cases": cases,
    }


def write_report(report, out):
    out.mkdir(parents=True, exist_ok=False)
    (out / "triage.json").write_text(json.dumps(report, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + "\n")
    rows = []
    for case in report["cases"]:
        row = {**case["key"], **{k: case[k] for k in (
            "default_bucket", "strict_bucket", "default_mismatch", "strict_only_mismatch", "bucket_changed",
            "primary_family", "classification_reason", "rule_yaml_path", "rule_yaml_sha256", "rule_git_blob")}}
        for field, value in case["traits"].items():
            row[field] = _json(value) if isinstance(value, (dict, list)) else value
        for field in ("matched_families", "metadata_paths", "suspected_causes"):
            row[field] = _json(case[field])
        row["evidence_kind"] = report["evidence_kind"]
        for mode, evidence in case["scoreboard_evidence"].items():
            for field in ("official_issue_count", "candidate_issue_count", "missing_count", "extra_count",
                          "official_results_csv", "candidate_report_csv", "issue_fingerprint_hash",
                          "reason", "scoring_policy", "execution_provenance", "execution_provenance_detail"):
                row[f"{mode}_{field}"] = evidence.get(field)
            for field in ("missing", "extra", "scoring_normalizations", "skipped_reasons"):
                row[f"{mode}_{field}"] = _json(evidence.get(field, []))
        rows.append(row)
    with (out / "triage.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    mismatch_columns = [
        *KEY_FIELDS, "primary_family", "rule_type", "sensitivity", "operators", "operations",
        "default_bucket", "default_official_issue_count", "default_candidate_issue_count",
        "default_missing_count", "default_extra_count", "rule_yaml_sha256",
    ]
    with (out / "mismatches.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=mismatch_columns, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(row for row in rows if row["default_mismatch"])
    summary = report["summary"]
    lines = ["# Candidate diagnostic triage", "", *report["limitations"], "",
             f"Cases: {summary['total_cases']}; default mismatches: {summary['default_mismatches']}; rules: {summary['default_mismatch_rules']}.",
             f"Strict-only match-to-mismatch transitions: {summary['strict_only_mismatches']}.", "",
             "## Default mismatch primary families", "", "| Family | Supported | Deferred | Total |", "| --- | ---: | ---: | ---: |"]
    for family, counts in summary["default_mismatch_families"].items():
        lines.append(f"| {family} | {counts['supported_mismatch']} | {counts['deferred_oracle_gap_mismatch']} | {counts['total']} |")
    lines += ["", "## Metadata reconciliation", ""]
    lines += [f"- {name}: {count}" for name, count in summary["default_mismatch_metadata_reconciliation"].items()]
    lines += ["", "## Buckets (skips and absent oracles are not mismatches)", "", "| Bucket | Default | Strict |", "| --- | ---: | ---: |"]
    lines += [f"| {b} | {summary['default_buckets'].get(b, 0)} | {summary['strict_buckets'].get(b, 0)} |" for b in BUCKETS]
    lines += ["", "## Bucket transitions", ""]
    lines += [f"- {name}: {count}" for name, count in summary["bucket_transitions"].items()]
    lines += ["", "## Default mismatch samples", ""]
    for family in summary["default_mismatch_families"]:
        sample = [c for c in report["cases"] if c["default_mismatch"] and c["primary_family"] == family][:3]
        labels = ["/".join(c["key"][f] for f in KEY_FIELDS) for c in sample]
        lines.append(f"- {family}: {', '.join(labels)}")
    (out / "triage.md").write_text("\n".join(lines) + "\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("default-scoreboard", "strict-scoreboard", "rules-root", "out"):
        parser.add_argument(f"--{flag}", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        report = build_report(args.default_scoreboard, args.strict_scoreboard, args.rules_root)
        write_report(report, args.out)
    except (InputError, OSError, ValueError, yaml.YAMLError, subprocess.CalledProcessError, RecursionError) as exc:
        print(f"triage: {exc}", file=sys.stderr)
        return 1
    print(_json(report["summary"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
