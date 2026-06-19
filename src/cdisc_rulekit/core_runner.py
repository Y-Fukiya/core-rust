from __future__ import annotations

import json
import shlex
from dataclasses import asdict, dataclass
from pathlib import Path

from .io_utils import ensure_dir

DEFAULT_ENGINE_COMMAND = "cargo run -p core-cli -- validate"


@dataclass(frozen=True)
class CoreRunPlanItem:
    generated_rule_id: str
    case_type: str
    case_id: str
    rule_dir: str
    rule_yml: str
    data_dir: str
    output_dir: str
    command: list[str]
    dry_run: bool

    def to_dict(self) -> dict[str, object]:
        return asdict(self)


@dataclass(frozen=True)
class CoreRunPlan:
    items: list[CoreRunPlanItem]
    dry_run: bool

    @property
    def case_count(self) -> int:
        return len(self.items)

    def to_dict(self) -> dict[str, object]:
        return {
            "dry_run": self.dry_run,
            "case_count": self.case_count,
            "items": [item.to_dict() for item in self.items],
        }


def _case_data_dirs(rule_dir: Path) -> list[tuple[str, str, Path]]:
    cases: list[tuple[str, str, Path]] = []
    for case_type in ("positive", "negative"):
        case_root = rule_dir / case_type
        if not case_root.exists():
            continue
        for data_dir in sorted(case_root.glob("*/data")):
            cases.append((case_type, data_dir.parent.name, data_dir))
    return cases


def _command(
    engine_command: str,
    rule_dir: Path,
    data_dir: Path,
    output_dir: Path,
) -> list[str]:
    return [
        *shlex.split(engine_command),
        "--local-rules",
        str(rule_dir),
        "--dataset-path",
        str(data_dir),
        "--output",
        str(output_dir),
    ]


def build_core_run_plan(
    generated_rules_dir: str | Path,
    run_root: str | Path,
    engine_command: str = DEFAULT_ENGINE_COMMAND,
    dry_run: bool = True,
) -> CoreRunPlan:
    generated_root = Path(generated_rules_dir)
    run_root_path = Path(run_root)
    items: list[CoreRunPlanItem] = []
    rule_dirs = sorted(path for path in generated_root.iterdir() if path.is_dir()) if generated_root.exists() else []

    for rule_dir in rule_dirs:
        rule_id = rule_dir.name
        rule_yml = rule_dir / "rule.yml"
        for case_type, case_id, data_dir in _case_data_dirs(rule_dir):
            output_dir = run_root_path / rule_id / case_type / case_id
            items.append(
                CoreRunPlanItem(
                    generated_rule_id=rule_id,
                    case_type=case_type,
                    case_id=case_id,
                    rule_dir=str(rule_dir),
                    rule_yml=str(rule_yml),
                    data_dir=str(data_dir),
                    output_dir=str(output_dir),
                    command=_command(engine_command, rule_dir, data_dir, output_dir),
                    dry_run=dry_run,
                ),
            )
    return CoreRunPlan(items=items, dry_run=dry_run)


def write_core_run_plan(out_dir: str | Path, plan: CoreRunPlan) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    (out / "core_run_plan.json").write_text(
        json.dumps(plan.to_dict(), ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    lines = [
        "# CORE Run Plan",
        "",
        f"- dry run: `{str(plan.dry_run).lower()}`",
        f"- planned cases: `{plan.case_count}`",
        "",
        "## Commands",
        "",
    ]
    if not plan.items:
        lines.append("- None")
    for item in plan.items:
        command = " ".join(shlex.quote(part) for part in item.command)
        lines.append(f"- `{item.generated_rule_id}` `{item.case_type}/{item.case_id}`")
        lines.append(f"  - `{command}`")
    lines.append("")
    (out / "core_run_plan.md").write_text("\n".join(lines), encoding="utf-8")
