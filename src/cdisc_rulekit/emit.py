from __future__ import annotations

from pathlib import Path

from .io_utils import write_csv, write_jsonl
from .models import CanonicalRule, OperatorInventoryItem, RuleMapping

P21_CATALOG_FIELDS = [
    "source",
    "source_rule_id",
    "p21_rule_id",
    "cdisc_rule_ids",
    "standard_name",
    "standard_version",
    "agency",
    "category",
    "severity",
    "p21_rule_type",
    "message",
    "description",
    "domains",
    "classes",
    "variables",
    "target",
    "raw_condition",
    "source_path",
    "raw_record",
]

CORE_CATALOG_FIELDS = [
    "source",
    "source_rule_id",
    "core_rule_id",
    "cdisc_rule_ids",
    "standard_name",
    "standard_version",
    "substandard",
    "core_rule_type",
    "message",
    "description",
    "domains",
    "classes",
    "variables",
    "raw_condition",
    "source_path",
]

TESTDATA_FIELDS = ["scope", "core_rule_id", "case_type", "case_id", "data_file"]

OPERATOR_FIELDS = ["core_rule_id", "source_path", "operator", "path", "node_kind", "name_values", "raw_keys"]

MAPPING_FIELDS = [
    "p21_rule_id",
    "core_rule_id",
    "match_type",
    "confidence",
    "cdisc_rule_id_overlap",
    "standard_match",
    "domain_overlap",
    "variable_overlap",
    "message_similarity",
    "notes",
]

CONVERSION_STATUS_FIELDS = [
    "source",
    "source_rule_id",
    "p21_rule_id",
    "core_rule_id",
    "cdisc_rule_ids",
    "standard_name",
    "standard_version",
    "domains",
    "variables",
    "p21_rule_type",
    "category",
    "conversion_status",
    "conversion_confidence",
    "conversion_reasons",
    "message",
    "source_path",
]


def emit_p21_catalog(out_dir: str | Path, rules: list[CanonicalRule]) -> None:
    out = Path(out_dir)
    rows = [rule.to_dict() for rule in rules]
    write_csv(out / "p21_rules_normalized.csv", rows, P21_CATALOG_FIELDS)
    write_jsonl(out / "p21_rules_normalized.jsonl", rows)


def emit_core_catalog(
    out_dir: str | Path,
    rules: list[CanonicalRule],
    testdata_inventory: list[dict[str, object]],
    operator_inventory: list[OperatorInventoryItem],
) -> None:
    out = Path(out_dir)
    rows = [rule.to_dict() for rule in rules]
    write_csv(out / "core_rules_normalized.csv", rows, CORE_CATALOG_FIELDS)
    write_jsonl(out / "core_rules_normalized.jsonl", rows)
    write_csv(out / "core_testdata_inventory.csv", testdata_inventory, TESTDATA_FIELDS)
    operator_rows = [item.to_dict() for item in operator_inventory]
    write_csv(out / "core_operator_inventory.csv", operator_rows, OPERATOR_FIELDS)
    write_jsonl(out / "core_operator_inventory.jsonl", operator_rows)


def emit_mapping(out_dir: str | Path, mappings: list[RuleMapping]) -> None:
    out = Path(out_dir)
    rows = [mapping.to_dict() for mapping in mappings]
    write_csv(out / "p21_to_core_mapping.csv", rows, MAPPING_FIELDS)
    write_jsonl(out / "p21_to_core_mapping.jsonl", rows)


def emit_conversion_status(out_dir: str | Path, rules: list[CanonicalRule]) -> None:
    out = Path(out_dir)
    rows = [rule.to_dict() for rule in rules]
    write_csv(out / "conversion_status.csv", rows, CONVERSION_STATUS_FIELDS)
    write_jsonl(out / "conversion_status.jsonl", rows)
