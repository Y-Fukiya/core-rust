from __future__ import annotations

import json
from collections import Counter
from pathlib import Path

from .io_utils import ensure_dir, write_csv
from .macro_analysis import macro_findings_for_rule
from .models import CanonicalRule, RuleMapping


def _status_counts(rules: list[CanonicalRule]) -> dict[str, int]:
    return dict(sorted(Counter(rule.conversion_status or "UNCLASSIFIED" for rule in rules).items()))


def _mapping_counts(mappings: list[RuleMapping]) -> dict[str, int]:
    return dict(sorted(Counter(mapping.match_type for mapping in mappings).items()))


def _reason_counts(rules: list[CanonicalRule]) -> dict[str, int]:
    counter: Counter[str] = Counter()
    for rule in rules:
        counter.update(rule.conversion_reasons)
    return dict(sorted(counter.items()))


def _mapping_by_key(mappings: list[RuleMapping]) -> dict[str, RuleMapping]:
    return {mapping.p21_rule_key or mapping.p21_rule_id: mapping for mapping in mappings}


def write_conversion_summary(
    report_dir: str | Path,
    classified_rules: list[CanonicalRule],
    warnings: list[str],
) -> None:
    out = Path(report_dir)
    ensure_dir(out)
    counts = _status_counts(classified_rules)
    lines = [
        "# Conversion Status Summary",
        "",
        "Read-only phase: no `rule.yml`, positive data, or negative data was generated.",
        "",
        "## Status Counts",
        "",
    ]
    for status, count in counts.items():
        lines.append(f"- `{status}`: {count}")
    lines.extend(["", "## Warnings", ""])
    if warnings:
        lines.extend(f"- {warning}" for warning in warnings)
    else:
        lines.append("- None")
    lines.append("")
    (out / "conversion_status_summary.md").write_text("\n".join(lines), encoding="utf-8")


def write_readiness_summary(
    report_dir: str | Path,
    classified_rules: list[CanonicalRule],
    mappings: list[RuleMapping],
    warnings: list[str],
) -> None:
    out = Path(report_dir)
    ensure_dir(out)
    payload = {
        "total_p21_rules": len(classified_rules),
        "status_counts": _status_counts(classified_rules),
        "mapping_counts": _mapping_counts(mappings),
        "generated_rules_created": 0,
        "warnings": warnings,
    }
    (out / "readiness_summary.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


MACRO_INVENTORY_FIELDS = [
    "source_rule_key",
    "p21_rule_id",
    "agency",
    "standard_name",
    "standard_version",
    "p21_rule_type",
    "conversion_status",
    "field",
    "macro_token",
    "macro_family",
    "convertibility_impact",
]

FUZZY_REVIEW_FIELDS = [
    "source_rule_key",
    "p21_rule_id",
    "agency",
    "standard_name",
    "standard_version",
    "p21_rule_type",
    "core_rule_id",
    "confidence",
    "standard_match",
    "domain_overlap",
    "variable_overlap",
    "message_similarity",
    "review_decision",
    "notes",
]

REASON_EXAMPLE_FIELDS = [
    "reason",
    "source_rule_key",
    "p21_rule_id",
    "agency",
    "standard_name",
    "standard_version",
    "p21_rule_type",
    "conversion_status",
    "core_rule_id",
    "message",
]

VERSION_AGENCY_FIELDS = [
    "standard_name",
    "standard_version",
    "agency",
    "conversion_status",
    "rule_count",
]

RAW_RULE_ID_FIELDS = [
    "p21_rule_id",
    "row_count",
    "standard_versions",
    "agencies",
    "conversion_status_counts",
    "mapping_type_counts",
]

SOURCE_TRACKING_FIELDS = [
    "source_rule_key",
    "p21_rule_id",
    "agency",
    "standard_name",
    "standard_version",
    "p21_rule_type",
    "conversion_status",
    "conversion_reasons",
    "core_rule_id",
    "mapping_type",
    "mapping_confidence",
]

BOUNDARY_REVIEW_FIELDS = [
    "boundary_bucket",
    "conversion_status",
    "primary_reason",
    "source_row_count",
    "raw_rule_id_count",
    "example_rule_ids",
    "recommended_action",
]


MAPPING_MARKER_REASONS = {"NO_CORE_MAPPING", "FUZZY_CORE_CANDIDATE", "HAS_NATIVE_CORE_MAPPING"}


def _primary_actionable_reason(rule: CanonicalRule) -> str:
    for reason in rule.conversion_reasons:
        if reason not in MAPPING_MARKER_REASONS:
            return reason
    return rule.conversion_reasons[0] if rule.conversion_reasons else "UNCLASSIFIED"


def _boundary_bucket(rule: CanonicalRule) -> str:
    status = rule.conversion_status or "UNCLASSIFIED"
    reasons = set(rule.conversion_reasons)
    if status == "NATIVE_CORE":
        return "NATIVE_CORE_MAPPING"
    if status == "AUTO_CONVERTIBLE":
        if "FUZZY_CORE_CANDIDATE" in reasons:
            return "AUTO_CONVERTIBLE_REVIEW_FUZZY_CORE_CANDIDATE"
        return "AUTO_CONVERTIBLE_READY"
    if status == "SKELETON_ONLY":
        if "NO_TARGET_VARIABLE" in reasons:
            return "SKELETON_MISSING_TARGET_VARIABLE"
        if "NO_CONCRETE_DOMAIN" in reasons:
            return "SKELETON_MISSING_CONCRETE_DOMAIN"
        return "SKELETON_METADATA_ONLY"
    if status == "MANUAL_REQUIRED":
        reason = _primary_actionable_reason(rule)
        if reason.startswith("P21_MACRO_") or reason == "UNRESOLVED_VARIABLE_MACRO":
            return "MANUAL_P21_MACRO_DEPENDENCY"
        if reason in {"DEFINE_XML_RULE", "SCHEMATRON_RULE"}:
            return "MANUAL_DEFINE_OR_SCHEMATRON"
        if reason == "METADATA_RULE":
            return "MANUAL_METADATA_DEPENDENCY"
        if reason == "EXTERNAL_LOOKUP_DEPENDENCY":
            return "MANUAL_EXTERNAL_LOOKUP_DEPENDENCY"
        if reason == "CROSS_DATASET_DEPENDENCY":
            return "MANUAL_CROSS_DATASET_DEPENDENCY"
        if reason == "UNSUPPORTED_RULE_TYPE":
            return "MANUAL_UNSUPPORTED_RULE_TYPE"
        return "MANUAL_REVIEW_REQUIRED"
    if status == "UNSUPPORTED":
        return "UNSUPPORTED_INPUT"
    return "UNCLASSIFIED"


def _recommended_action(boundary_bucket: str) -> str:
    if boundary_bucket == "NATIVE_CORE_MAPPING":
        return "Track as existing CORE coverage; no generated draft rule."
    if boundary_bucket == "AUTO_CONVERTIBLE_READY":
        return "Generate draft rule and deterministic positive/negative data."
    if boundary_bucket == "AUTO_CONVERTIBLE_REVIEW_FUZZY_CORE_CANDIDATE":
        return "Generate only as draft P21PORT rule; review fuzzy CORE candidate before export."
    if boundary_bucket == "SKELETON_MISSING_TARGET_VARIABLE":
        return "Keep skeleton until target variable or parser support is confirmed."
    if boundary_bucket == "SKELETON_MISSING_CONCRETE_DOMAIN":
        return "Keep skeleton until dataset/domain scope can be derived safely."
    if boundary_bucket.startswith("MANUAL_"):
        return "Do not auto-generate; requires human rule design or engine capability work."
    if boundary_bucket == "UNSUPPORTED_INPUT":
        return "Repair or exclude malformed source input."
    return "Review classification."


def _write_boundary_review(out: Path, classified_rules: list[CanonicalRule]) -> None:
    grouped: dict[tuple[str, str, str], list[CanonicalRule]] = {}
    for rule in classified_rules:
        bucket = _boundary_bucket(rule)
        key = (bucket, rule.conversion_status or "UNCLASSIFIED", _primary_actionable_reason(rule))
        grouped.setdefault(key, []).append(rule)

    rows: list[dict[str, object]] = []
    for (bucket, status, reason), rules in sorted(grouped.items()):
        example_ids = sorted({rule.p21_rule_id or rule.source_rule_id for rule in rules if rule.p21_rule_id or rule.source_rule_id})
        rows.append(
            {
                "boundary_bucket": bucket,
                "conversion_status": status,
                "primary_reason": reason,
                "source_row_count": len(rules),
                "raw_rule_id_count": len({rule.p21_rule_id or rule.source_rule_id for rule in rules}),
                "example_rule_ids": example_ids[:10],
                "recommended_action": _recommended_action(bucket),
            },
        )
    write_csv(out / "classification_boundary_review.csv", rows, BOUNDARY_REVIEW_FIELDS)

    lines = [
        "# Classification Boundary Review",
        "",
        "This report fixes the Phase 1 boundary between automatic generation, skeleton output, and manual work.",
        "Counts are source rows; `raw_rule_id_count` shows the distinct P21 rule IDs represented by those rows.",
        "",
        "## Boundary Buckets",
        "",
    ]
    for row in rows:
        lines.append(
            "- "
            f"`{row['boundary_bucket']}` / `{row['primary_reason']}`: "
            f"{row['source_row_count']} rows, {row['raw_rule_id_count']} raw rule IDs. "
            f"{row['recommended_action']}"
        )
    lines.append("")
    (out / "classification_boundary_review.md").write_text("\n".join(lines), encoding="utf-8")


def _write_classification_quality(
    out: Path,
    classified_rules: list[CanonicalRule],
    mappings: list[RuleMapping],
    core_rule_count: int | None,
    testdata_file_count: int | None,
) -> None:
    lines = [
        "# Phase 1 Classification Quality",
        "",
        "Read-only phase: no `rule.yml`, positive data, or negative data was generated.",
        "",
        "## Input Coverage",
        "",
        f"- P21 rows: `{len(classified_rules)}`",
        f"- Unique P21 raw rule IDs: `{len({rule.p21_rule_id for rule in classified_rules})}`",
        f"- Unique P21 source rule keys: `{len({rule.source_rule_key for rule in classified_rules})}`",
    ]
    if core_rule_count is not None:
        lines.append(f"- CORE Published rules after standard filter: `{core_rule_count}`")
    if testdata_file_count is not None:
        lines.append(f"- CORE test data files after standard filter: `{testdata_file_count}`")
    lines.extend(["", "## Status Counts", ""])
    for status, count in _status_counts(classified_rules).items():
        lines.append(f"- `{status}`: {count}")
    lines.extend(["", "## Mapping Counts", ""])
    for match_type, count in _mapping_counts(mappings).items():
        lines.append(f"- `{match_type}`: {count}")
    lines.extend(["", "## Reason Counts", ""])
    for reason, count in _reason_counts(classified_rules).items():
        lines.append(f"- `{reason}`: {count}")
    lines.append("")
    (out / "classification_quality.md").write_text("\n".join(lines), encoding="utf-8")


def _write_macro_inventory(out: Path, classified_rules: list[CanonicalRule]) -> None:
    rows: list[dict[str, object]] = []
    for rule in classified_rules:
        for finding in macro_findings_for_rule(rule):
            row = finding.to_dict()
            row["conversion_status"] = rule.conversion_status
            rows.append(row)
    write_csv(out / "macro_inventory.csv", rows, MACRO_INVENTORY_FIELDS)

    family_counts = Counter(row["macro_family"] for row in rows)
    impact_counts = Counter(row["convertibility_impact"] for row in rows)
    lines = [
        "# Macro Inventory Summary",
        "",
        "Macros found in P21 fields are reported separately from conversion classification.",
        "",
        "## Family Counts",
        "",
    ]
    lines.extend(f"- `{family}`: {count}" for family, count in sorted(family_counts.items()))
    if not family_counts:
        lines.append("- None")
    lines.extend(["", "## Convertibility Impact Counts", ""])
    lines.extend(f"- `{impact}`: {count}" for impact, count in sorted(impact_counts.items()))
    if not impact_counts:
        lines.append("- None")
    lines.append("")
    (out / "macro_inventory_summary.md").write_text("\n".join(lines), encoding="utf-8")


def _write_fuzzy_review(out: Path, classified_rules: list[CanonicalRule], mappings: list[RuleMapping]) -> None:
    rules_by_key = {rule.source_rule_key or rule.source_rule_id: rule for rule in classified_rules}
    rows: list[dict[str, object]] = []
    for mapping in mappings:
        if mapping.match_type != "FUZZY":
            continue
        rule = rules_by_key.get(mapping.p21_rule_key or mapping.p21_rule_id)
        rows.append(
            {
                "source_rule_key": mapping.p21_rule_key,
                "p21_rule_id": mapping.p21_rule_id,
                "agency": rule.agency if rule else None,
                "standard_name": rule.standard_name if rule else None,
                "standard_version": rule.standard_version if rule else None,
                "p21_rule_type": rule.p21_rule_type if rule else None,
                "core_rule_id": mapping.core_rule_id,
                "confidence": mapping.confidence,
                "standard_match": mapping.standard_match,
                "domain_overlap": mapping.domain_overlap,
                "variable_overlap": mapping.variable_overlap,
                "message_similarity": mapping.message_similarity,
                "review_decision": "REVIEW_ONLY_NOT_NATIVE",
                "notes": mapping.notes,
            },
        )
    write_csv(out / "fuzzy_mapping_review.csv", rows, FUZZY_REVIEW_FIELDS)


def _write_reason_examples(out: Path, classified_rules: list[CanonicalRule], examples_per_reason: int = 5) -> None:
    rows: list[dict[str, object]] = []
    seen: Counter[str] = Counter()
    for rule in classified_rules:
        for reason in rule.conversion_reasons:
            if seen[reason] >= examples_per_reason:
                continue
            seen[reason] += 1
            rows.append(
                {
                    "reason": reason,
                    "source_rule_key": rule.source_rule_key,
                    "p21_rule_id": rule.p21_rule_id,
                    "agency": rule.agency,
                    "standard_name": rule.standard_name,
                    "standard_version": rule.standard_version,
                    "p21_rule_type": rule.p21_rule_type,
                    "conversion_status": rule.conversion_status,
                    "core_rule_id": rule.core_rule_id,
                    "message": rule.message,
                },
            )
    write_csv(out / "reason_examples.csv", rows, REASON_EXAMPLE_FIELDS)


def _write_version_agency_summary(out: Path, classified_rules: list[CanonicalRule]) -> None:
    counter: Counter[tuple[str | None, str | None, str | None, str | None]] = Counter()
    for rule in classified_rules:
        counter[(rule.standard_name, rule.standard_version, rule.agency, rule.conversion_status)] += 1
    rows = [
        {
            "standard_name": standard_name,
            "standard_version": standard_version,
            "agency": agency,
            "conversion_status": status,
            "rule_count": count,
        }
        for (standard_name, standard_version, agency, status), count in sorted(counter.items())
    ]
    write_csv(out / "version_agency_summary.csv", rows, VERSION_AGENCY_FIELDS)


def _write_rule_tracking(out: Path, classified_rules: list[CanonicalRule], mappings: list[RuleMapping]) -> None:
    mapping_by_key = _mapping_by_key(mappings)
    grouped: dict[str, list[CanonicalRule]] = {}
    for rule in classified_rules:
        grouped.setdefault(rule.p21_rule_id or rule.source_rule_id, []).append(rule)

    raw_rows: list[dict[str, object]] = []
    for p21_rule_id, rules in sorted(grouped.items()):
        mapping_counter: Counter[str] = Counter()
        status_counter: Counter[str] = Counter()
        for rule in rules:
            status_counter[rule.conversion_status or "UNCLASSIFIED"] += 1
            mapping = mapping_by_key.get(rule.source_rule_key or rule.source_rule_id)
            mapping_counter[mapping.match_type if mapping else "NONE"] += 1
        raw_rows.append(
            {
                "p21_rule_id": p21_rule_id,
                "row_count": len(rules),
                "standard_versions": sorted({rule.standard_version for rule in rules if rule.standard_version}),
                "agencies": sorted({rule.agency for rule in rules if rule.agency}),
                "conversion_status_counts": dict(sorted(status_counter.items())),
                "mapping_type_counts": dict(sorted(mapping_counter.items())),
            },
        )
    write_csv(out / "raw_rule_id_summary.csv", raw_rows, RAW_RULE_ID_FIELDS)

    source_rows: list[dict[str, object]] = []
    for rule in classified_rules:
        mapping = mapping_by_key.get(rule.source_rule_key or rule.source_rule_id)
        source_rows.append(
            {
                "source_rule_key": rule.source_rule_key,
                "p21_rule_id": rule.p21_rule_id,
                "agency": rule.agency,
                "standard_name": rule.standard_name,
                "standard_version": rule.standard_version,
                "p21_rule_type": rule.p21_rule_type,
                "conversion_status": rule.conversion_status,
                "conversion_reasons": rule.conversion_reasons,
                "core_rule_id": rule.core_rule_id,
                "mapping_type": mapping.match_type if mapping else None,
                "mapping_confidence": mapping.confidence if mapping else None,
            },
        )
    write_csv(out / "source_rule_tracking.csv", source_rows, SOURCE_TRACKING_FIELDS)


def write_phase1_quality_reports(
    report_dir: str | Path,
    classified_rules: list[CanonicalRule],
    mappings: list[RuleMapping],
    core_rule_count: int | None = None,
    testdata_file_count: int | None = None,
) -> None:
    out = Path(report_dir)
    ensure_dir(out)
    _write_classification_quality(out, classified_rules, mappings, core_rule_count, testdata_file_count)
    _write_macro_inventory(out, classified_rules)
    _write_fuzzy_review(out, classified_rules, mappings)
    _write_reason_examples(out, classified_rules)
    _write_version_agency_summary(out, classified_rules)
    _write_rule_tracking(out, classified_rules, mappings)
    _write_boundary_review(out, classified_rules)
