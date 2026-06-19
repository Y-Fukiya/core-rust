from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from .io_utils import normalize_blank
from .models import CanonicalRule

try:
    import yaml
except ImportError:  # pragma: no cover - exercised only when dependency is absent.
    yaml = None


CG_RE = re.compile(r"\bCG\d{4,}\b")


def discover_rule_yml(repo_path: str | Path, include_unpublished: bool = False) -> list[Path]:
    repo = Path(repo_path)
    roots = [repo / "Published"]
    if include_unpublished:
        roots.append(repo / "Unpublished")
    paths: list[Path] = []
    for root in roots:
        if root.exists():
            paths.extend(root.rglob("rule.yml"))
    return sorted(paths)


def _require_yaml() -> Any:
    if yaml is None:
        raise RuntimeError("PyYAML is required to parse CDISC Open Rules rule.yml files")
    return yaml


def _get(mapping: dict[str, Any], *keys: str) -> Any:
    current: Any = mapping
    for key in keys:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def _listify(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, list):
        return sorted({str(item).upper() for item in value if normalize_blank(item)})
    text = normalize_blank(value)
    return [text.upper()] if text else []


def _extract_cg_ids_from_text(text: str) -> list[str]:
    return sorted({match.group(0).upper() for match in CG_RE.finditer(text)})


def _authority_fields(data: dict[str, Any]) -> tuple[str | None, str | None, str | None, list[str]]:
    standard_name: str | None = None
    standard_version: str | None = None
    substandard: str | None = None
    cdisc_ids: set[str] = set()
    for authority in data.get("Authorities") or []:
        if not isinstance(authority, dict):
            continue
        for standard in authority.get("Standards") or []:
            if not isinstance(standard, dict):
                continue
            standard_name = standard_name or normalize_blank(standard.get("Name"))
            standard_version = standard_version or normalize_blank(standard.get("Version"))
            substandard = substandard or normalize_blank(standard.get("Substandard"))
            for reference in standard.get("References") or []:
                if not isinstance(reference, dict):
                    continue
                identifier = _get(reference, "Rule Identifier", "Id")
                cdisc_ids.update(_extract_cg_ids_from_text(str(identifier or "")))
    return standard_name, standard_version, substandard, sorted(cdisc_ids)


def _collect_names(node: Any) -> list[str]:
    names: set[str] = set()
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "name":
                text = normalize_blank(value)
                if text:
                    names.add(text.upper())
            else:
                names.update(_collect_names(value))
    elif isinstance(node, list):
        for item in node:
            names.update(_collect_names(item))
    return sorted(names)


def _scope_values(data: dict[str, Any], kind: str) -> list[str]:
    include = _get(data, "Scope", kind, "Include")
    return _listify(include)


def _scope_name(path: Path, repo: Path) -> str:
    try:
        return path.relative_to(repo).parts[0]
    except ValueError:
        return ""


def inventory_testdata(rule_dir: Path, repo: Path, core_rule_id: str) -> list[dict[str, object]]:
    records: list[dict[str, object]] = []
    scope = _scope_name(rule_dir, repo)
    for case_type in ("positive", "negative"):
        root = rule_dir / case_type
        if not root.exists():
            continue
        for data_dir in sorted(root.glob("*/data")):
            case_id = data_dir.parent.name
            for path in sorted(data_dir.glob("*.csv")):
                if path.name.startswith("_"):
                    continue
                records.append(
                    {
                        "scope": scope,
                        "core_rule_id": core_rule_id,
                        "case_type": case_type,
                        "case_id": case_id,
                        "data_file": path.name,
                    }
                )
    return records


def _rule_from_yaml(path: Path, repo: Path, data: dict[str, Any], raw_text: str) -> CanonicalRule:
    core_rule_id = normalize_blank(_get(data, "Core", "Id")) or path.parent.name
    standard_name, standard_version, substandard, authority_ids = _authority_fields(data)
    fallback_ids = _extract_cg_ids_from_text(raw_text)
    cdisc_ids = sorted(set(authority_ids) | set(fallback_ids))
    check = data.get("Check") if isinstance(data.get("Check"), (dict, list)) else {}

    return CanonicalRule(
        source="CDISC_OPEN_RULES",
        source_rule_id=core_rule_id,
        core_rule_id=core_rule_id,
        cdisc_rule_ids=cdisc_ids,
        standard_name=standard_name,
        standard_version=standard_version,
        substandard=substandard,
        core_rule_type=normalize_blank(data.get("Rule Type")),
        message=normalize_blank(_get(data, "Outcome", "Message")),
        description=normalize_blank(_get(data, "Core", "Description")),
        domains=_scope_values(data, "Domains"),
        classes=_scope_values(data, "Classes"),
        variables=_collect_names(check),
        raw_condition={"Check": check},
        source_path=str(path),
        raw_record=data,
    )


def load_open_rules(
    repo_path: str | Path,
    include_unpublished: bool = False,
) -> tuple[list[CanonicalRule], list[dict[str, object]], list[str]]:
    parser = _require_yaml()
    repo = Path(repo_path)
    rules: list[CanonicalRule] = []
    testdata_inventory: list[dict[str, object]] = []
    warnings: list[str] = []

    for path in discover_rule_yml(repo, include_unpublished=include_unpublished):
        raw_text = path.read_text(encoding="utf-8")
        try:
            loaded = parser.safe_load(raw_text) or {}
        except Exception as error:  # noqa: BLE001 - warning should include parser details.
            warnings.append(f"{path}: malformed YAML: {error}")
            continue
        if not isinstance(loaded, dict):
            warnings.append(f"{path}: YAML root is not a mapping")
            continue
        rule = _rule_from_yaml(path, repo, loaded, raw_text)
        rules.append(rule)
        testdata_inventory.extend(inventory_testdata(path.parent, repo, rule.core_rule_id or path.parent.name))

    return rules, testdata_inventory, warnings
