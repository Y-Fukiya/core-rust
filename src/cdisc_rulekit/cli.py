from __future__ import annotations

import argparse
import json
from pathlib import Path

from .classify import classify_rules
from .emit import emit_conversion_status, emit_core_catalog, emit_mapping, emit_p21_catalog
from .inputs import resolve_open_rules_input
from .io_utils import read_jsonl
from .load_open_rules import load_open_rules
from .load_p21 import load_p21_rules
from .map_rules import map_p21_to_core, standard_key
from .models import CanonicalRule, RuleMapping
from .operator_inventory import build_operator_inventory
from .reports import write_conversion_summary, write_phase1_quality_reports, write_readiness_summary


def _filter_by_standard(rules: list[CanonicalRule], standard: str | None) -> list[CanonicalRule]:
    requested = standard_key(standard)
    if not requested:
        return rules

    def rule_matches(rule: CanonicalRule) -> bool:
        values = [
            rule.standard_name,
            rule.substandard,
            rule.raw_record.get("config_name"),
            rule.raw_record.get("standard_name"),
        ]
        return requested in {standard_key(str(value)) for value in values if value}

    return [rule for rule in rules if rule_matches(rule)]


def _limit_rules(rules: list[CanonicalRule], limit: int | None) -> list[CanonicalRule]:
    if limit is None:
        return rules
    if limit < 0:
        raise ValueError("--limit must be zero or greater")
    return rules[:limit]


def _filter_testdata_inventory(
    inventory: list[dict[str, object]],
    core_rules: list[CanonicalRule],
) -> list[dict[str, object]]:
    core_rule_ids = {rule.core_rule_id for rule in core_rules if rule.core_rule_id}
    return [row for row in inventory if row.get("core_rule_id") in core_rule_ids]


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
    repo = resolve_open_rules_input(args.repo, Path(args.out).parent / "_work")
    rules, testdata_inventory, warnings = load_open_rules(
        repo,
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
    write_phase1_quality_reports(args.reports, classified, mappings)
    print(f"classify complete: {len(classified)} rules")
    return 0


def cmd_build_readonly(args: argparse.Namespace) -> int:
    p21_rules, p21_warnings = load_p21_rules(args.p21_rules, args.p21_domain_map)
    p21_rules = _limit_rules(_filter_by_standard(p21_rules, args.standard), args.limit)
    root = Path(args.out)
    open_rules_repo = resolve_open_rules_input(args.open_rules_repo, root / "_work")
    core_rules, testdata_inventory, core_warnings = load_open_rules(
        open_rules_repo,
        include_unpublished=args.include_unpublished,
    )
    core_rules = _filter_by_standard(core_rules, args.standard)
    testdata_inventory = _filter_testdata_inventory(testdata_inventory, core_rules)
    operator_inventory = build_operator_inventory(core_rules)
    mappings = map_p21_to_core(p21_rules, core_rules)
    classified = classify_rules(p21_rules, mappings)

    emit_p21_catalog(root / "catalog", p21_rules)
    emit_core_catalog(root / "catalog", core_rules, testdata_inventory, operator_inventory)
    emit_mapping(root / "mapping", mappings)
    emit_conversion_status(root / "catalog", classified)
    warnings = p21_warnings + core_warnings
    write_conversion_summary(root / "reports", classified, warnings)
    write_readiness_summary(root / "reports", classified, mappings, warnings)
    write_phase1_quality_reports(
        root / "reports",
        classified,
        mappings,
        core_rule_count=len(core_rules),
        testdata_file_count=len(testdata_inventory),
    )

    print(f"build-readonly complete: {len(classified)} P21 rules, {len(core_rules)} CORE rules")
    return 0


def _preflight_work_root(out: Path) -> Path:
    return out.parent / "_work" if out.name == "reports" else out / "_work"


def cmd_pilot_preflight(args: argparse.Namespace) -> int:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    warnings: list[str] = []
    p21_rules: list[CanonicalRule] = []
    core_rules: list[CanonicalRule] = []
    testdata_inventory: list[dict[str, object]] = []
    resolved_open_rules_repo: Path | None = None

    try:
        p21_rules, p21_warnings = load_p21_rules(args.p21_rules, args.p21_domain_map)
        warnings.extend(p21_warnings)
    except Exception as error:  # noqa: BLE001 - preflight reports input readiness.
        errors.append(f"P21 input could not be loaded: {error}")

    try:
        resolved_open_rules_repo = resolve_open_rules_input(args.open_rules_repo, _preflight_work_root(out))
        core_rules, testdata_inventory, core_warnings = load_open_rules(
            resolved_open_rules_repo,
            include_unpublished=args.include_unpublished,
        )
        warnings.extend(core_warnings)
    except Exception as error:  # noqa: BLE001 - preflight reports input readiness.
        errors.append(f"CDISC Open Rules input could not be loaded: {error}")

    p21_rules = _limit_rules(_filter_by_standard(p21_rules, args.standard), args.limit)
    core_rules = _filter_by_standard(core_rules, args.standard)
    testdata_inventory = _filter_testdata_inventory(testdata_inventory, core_rules)
    published_rule_count = sum(1 for rule in core_rules if "/Published/" in f"/{rule.source_path or ''}")
    unpublished_rule_count = sum(1 for rule in core_rules if "/Unpublished/" in f"/{rule.source_path or ''}")
    payload = {
        "ok": not errors and bool(p21_rules) and bool(published_rule_count),
        "p21_rules_path": str(args.p21_rules),
        "p21_domain_map_path": str(args.p21_domain_map) if args.p21_domain_map else None,
        "open_rules_input_path": str(args.open_rules_repo),
        "open_rules_resolved_path": str(resolved_open_rules_repo) if resolved_open_rules_repo else None,
        "standard": args.standard,
        "limit": args.limit,
        "p21_rule_count": len(p21_rules),
        "open_rules_published_rule_yml_count": published_rule_count,
        "open_rules_unpublished_rule_yml_count": unpublished_rule_count,
        "open_rules_testdata_file_count": len(testdata_inventory),
        "warnings": warnings,
        "errors": errors,
    }
    (out / "pilot_preflight.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    lines = [
        "# Pilot Preflight",
        "",
        f"- ok: `{str(payload['ok']).lower()}`",
        f"- P21 rules: `{payload['p21_rule_count']}`",
        f"- Open Rules Published rule.yml: `{payload['open_rules_published_rule_yml_count']}`",
        f"- Open Rules test data files: `{payload['open_rules_testdata_file_count']}`",
        "",
        "## Errors",
        "",
    ]
    lines.extend(f"- {error}" for error in errors) if errors else lines.append("- None")
    lines.extend(["", "## Warnings", ""])
    lines.extend(f"- {warning}" for warning in warnings) if warnings else lines.append("- None")
    lines.append("")
    (out / "pilot_preflight.md").write_text("\n".join(lines), encoding="utf-8")

    print(
        "pilot-preflight complete: "
        f"{payload['p21_rule_count']} P21 rules, "
        f"{payload['open_rules_published_rule_yml_count']} Published CORE rules"
    )
    return 0 if payload["ok"] else 1


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

    preflight = subcommands.add_parser("pilot-preflight")
    preflight.add_argument("--p21-rules", type=Path, required=True)
    preflight.add_argument("--p21-domain-map", type=Path, default=None)
    preflight.add_argument("--open-rules-repo", type=Path, required=True)
    preflight.add_argument("--out", type=Path, required=True)
    preflight.add_argument("--standard", default=None)
    preflight.add_argument("--limit", type=int, default=None)
    preflight.add_argument("--include-unpublished", action="store_true")
    preflight.set_defaults(func=cmd_pilot_preflight)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
