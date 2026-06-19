from __future__ import annotations

import argparse
from pathlib import Path

from .classify import classify_rules
from .emit import emit_conversion_status, emit_core_catalog, emit_mapping, emit_p21_catalog
from .io_utils import read_jsonl
from .load_open_rules import load_open_rules
from .load_p21 import load_p21_rules
from .map_rules import map_p21_to_core
from .models import CanonicalRule, RuleMapping
from .operator_inventory import build_operator_inventory
from .reports import write_conversion_summary, write_readiness_summary


def _canonical_rules_from_jsonl(path: Path) -> list[CanonicalRule]:
    return [CanonicalRule.from_dict(row) for row in read_jsonl(path)]


def _mappings_from_jsonl(path: Path) -> list[RuleMapping]:
    return [RuleMapping.from_dict(row) for row in read_jsonl(path)]


def cmd_ingest_p21(args: argparse.Namespace) -> int:
    rules, warnings = load_p21_rules(args.rules, args.domain_map)
    emit_p21_catalog(args.out, rules)
    for warning in warnings:
        print(f"warning: {warning}")
    print(f"ingest-p21 complete: {len(rules)} rules")
    return 0


def cmd_ingest_open_rules(args: argparse.Namespace) -> int:
    rules, testdata_inventory, warnings = load_open_rules(
        args.repo,
        include_unpublished=args.include_unpublished,
    )
    operator_inventory = build_operator_inventory(rules)
    emit_core_catalog(args.out, rules, testdata_inventory, operator_inventory)
    for warning in warnings:
        print(f"warning: {warning}")
    print(f"ingest-open-rules complete: {len(rules)} rules")
    return 0


def cmd_map(args: argparse.Namespace) -> int:
    p21_rules = _canonical_rules_from_jsonl(args.p21)
    core_rules = _canonical_rules_from_jsonl(args.core)
    mappings = map_p21_to_core(p21_rules, core_rules)
    emit_mapping(args.out, mappings)
    print(f"map complete: {len(mappings)} mappings")
    return 0


def cmd_classify(args: argparse.Namespace) -> int:
    p21_rules = _canonical_rules_from_jsonl(args.p21)
    mappings = _mappings_from_jsonl(args.mapping)
    classified = classify_rules(p21_rules, mappings)
    emit_conversion_status(args.out, classified)
    write_conversion_summary(args.reports, classified, [])
    write_readiness_summary(args.reports, classified, mappings, [])
    print(f"classify complete: {len(classified)} rules")
    return 0


def cmd_build_readonly(args: argparse.Namespace) -> int:
    p21_rules, p21_warnings = load_p21_rules(args.p21_rules, args.p21_domain_map)
    core_rules, testdata_inventory, core_warnings = load_open_rules(
        args.open_rules_repo,
        include_unpublished=args.include_unpublished,
    )
    operator_inventory = build_operator_inventory(core_rules)
    mappings = map_p21_to_core(p21_rules, core_rules)
    classified = classify_rules(p21_rules, mappings)

    root = Path(args.out)
    emit_p21_catalog(root / "catalog", p21_rules)
    emit_core_catalog(root / "catalog", core_rules, testdata_inventory, operator_inventory)
    emit_mapping(root / "mapping", mappings)
    emit_conversion_status(root / "catalog", classified)
    warnings = p21_warnings + core_warnings
    write_conversion_summary(root / "reports", classified, warnings)
    write_readiness_summary(root / "reports", classified, mappings, warnings)

    print(f"build-readonly complete: {len(classified)} P21 rules, {len(core_rules)} CORE rules")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="cdisc-rulekit")
    subcommands = parser.add_subparsers(dest="command", required=True)

    ingest_p21 = subcommands.add_parser("ingest-p21")
    ingest_p21.add_argument("--rules", type=Path, required=True)
    ingest_p21.add_argument("--domain-map", type=Path, default=None)
    ingest_p21.add_argument("--out", type=Path, required=True)
    ingest_p21.set_defaults(func=cmd_ingest_p21)

    ingest_open = subcommands.add_parser("ingest-open-rules")
    ingest_open.add_argument("--repo", type=Path, required=True)
    ingest_open.add_argument("--out", type=Path, required=True)
    ingest_open.add_argument("--include-unpublished", action="store_true")
    ingest_open.set_defaults(func=cmd_ingest_open_rules)

    map_command = subcommands.add_parser("map")
    map_command.add_argument("--p21", type=Path, required=True)
    map_command.add_argument("--core", type=Path, required=True)
    map_command.add_argument("--out", type=Path, required=True)
    map_command.set_defaults(func=cmd_map)

    classify = subcommands.add_parser("classify")
    classify.add_argument("--p21", type=Path, required=True)
    classify.add_argument("--mapping", type=Path, required=True)
    classify.add_argument("--out", type=Path, required=True)
    classify.add_argument("--reports", type=Path, required=True)
    classify.set_defaults(func=cmd_classify)

    build = subcommands.add_parser("build-readonly")
    build.add_argument("--p21-rules", type=Path, required=True)
    build.add_argument("--p21-domain-map", type=Path, default=None)
    build.add_argument("--open-rules-repo", type=Path, required=True)
    build.add_argument("--out", type=Path, required=True)
    build.add_argument("--standard", default=None)
    build.add_argument("--limit", type=int, default=None)
    build.add_argument("--include-unpublished", action="store_true")
    build.set_defaults(func=cmd_build_readonly)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
