from __future__ import annotations

import json
import shlex
import subprocess
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path

from .io_utils import ensure_dir, write_csv

DEFAULT_ENGINE_COMMAND = "cargo run -p core-cli -- validate"
EXECUTION_FIELDS = [
    "generated_rule_id",
    "case_type",
    "case_id",
    "returncode",
    "status",
    "stdout",
    "stderr",
    "output_dir",
]


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


@dataclass(frozen=True)
class CoreRunExecutionResult:
    rows: list[dict[str, object]]

    @property
    def pass_count(self) -> int:
        return sum(1 for row in self.rows if row["status"] == "PASS")

    @property
    def fail_count(self) -> int:
        return sum(1 for row in self.rows if row["status"] == "FAIL")

    @property
    def ok(self) -> bool:
        return self.fail_count == 0


def _case_data_dirs(rule_dir: Path) -> list[tuple[str, str, Path]]:
    cases: list[tuple[str, str, Path]] = []
    for case_type in ("positive", "negative"):
        case_root = rule_dir / case_type
        if not case_root.exists():
            continue
        for data_dir in sorted(case_root.glob("*/data")):
            cases.append((case_type, data_dir.parent.name, data_dir))
    return cases


def _dataset_csv_paths(data_dir: Path) -> list[Path]:
    return sorted(
        path
        for path in data_dir.glob("*.csv")
        if path.is_file() and not path.name.startswith("_")
    )


def _command(
    engine_command: str,
    rule_dir: Path,
    data_dir: Path,
    output_dir: Path,
    output_mode: str = "directory",
    data_mode: str = "dataset-paths",
) -> list[str]:
    if output_mode not in {"directory", "file-base"}:
        raise ValueError(f"Unsupported output mode: {output_mode}")
    if data_mode not in {"dataset-paths", "data-dir"}:
        raise ValueError(f"Unsupported data mode: {data_mode}")
    output_argument = output_dir / "report" if output_mode == "file-base" else output_dir
    command = [
        *shlex.split(engine_command),
        "--local-rules",
        str(rule_dir / "rule.yml"),
    ]
    if data_mode == "data-dir":
        command.extend(["--data", str(data_dir)])
    else:
        for dataset_path in _dataset_csv_paths(data_dir):
            command.extend(["--dataset-path", str(dataset_path)])
    command.extend(
        [
            "--output",
            str(output_argument),
        ],
    )
    return [
        *command,
    ]


def build_core_run_plan(
    generated_rules_dir: str | Path,
    run_root: str | Path,
    engine_command: str = DEFAULT_ENGINE_COMMAND,
    dry_run: bool = True,
    output_mode: str = "directory",
    data_mode: str = "dataset-paths",
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
                    command=_command(engine_command, rule_dir, data_dir, output_dir, output_mode, data_mode),
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


def _execute_core_run_item(item: CoreRunPlanItem, cwd: str | None) -> dict[str, object]:
    ensure_dir(Path(item.output_dir))
    return _execute_core_run_item_with_timeout(item, cwd, timeout_seconds=None)


def _execute_core_run_item_with_timeout(
    item: CoreRunPlanItem,
    cwd: str | None,
    timeout_seconds: float | None,
) -> dict[str, object]:
    ensure_dir(Path(item.output_dir))
    try:
        completed = subprocess.run(
            item.command,
            check=False,
            capture_output=True,
            text=True,
            cwd=cwd,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        stdout = error.stdout or ""
        stderr = error.stderr or ""
        timeout_text = f"command timed out after {timeout_seconds:g}s"
        return {
            "generated_rule_id": item.generated_rule_id,
            "case_type": item.case_type,
            "case_id": item.case_id,
            "returncode": "TIMEOUT",
            "status": "FAIL",
            "stdout": stdout.strip() if isinstance(stdout, str) else "",
            "stderr": f"{stderr.strip()}\n{timeout_text}".strip() if isinstance(stderr, str) else timeout_text,
            "output_dir": item.output_dir,
        }
    return {
        "generated_rule_id": item.generated_rule_id,
        "case_type": item.case_type,
        "case_id": item.case_id,
        "returncode": completed.returncode,
        "status": "PASS" if completed.returncode == 0 else "FAIL",
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
        "output_dir": item.output_dir,
    }


def execute_core_run_plan(
    plan: CoreRunPlan,
    engine_cwd: str | Path | None = None,
    workers: int = 1,
    timeout_seconds: float | None = None,
) -> CoreRunExecutionResult:
    cwd = str(engine_cwd) if engine_cwd else None
    if workers <= 1:
        rows = [_execute_core_run_item_with_timeout(item, cwd, timeout_seconds) for item in plan.items]
        return CoreRunExecutionResult(rows)

    with ThreadPoolExecutor(max_workers=workers) as executor:
        rows = list(executor.map(lambda item: _execute_core_run_item_with_timeout(item, cwd, timeout_seconds), plan.items))
    return CoreRunExecutionResult(rows)


def write_core_run_execution_report(out_dir: str | Path, result: CoreRunExecutionResult) -> None:
    out = Path(out_dir)
    ensure_dir(out)
    write_csv(out / "core_run_execution_summary.csv", result.rows, EXECUTION_FIELDS)
    (out / "core_run_execution_summary.json").write_text(
        json.dumps(
            {
                "ok": result.ok,
                "pass_count": result.pass_count,
                "fail_count": result.fail_count,
                "rows": result.rows,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    lines = [
        "# CORE Run Execution Summary",
        "",
        f"- ok: `{str(result.ok).lower()}`",
        f"- passed rows: `{result.pass_count}`",
        f"- failed rows: `{result.fail_count}`",
        "",
        "## Failures",
        "",
    ]
    failures = [row for row in result.rows if row["status"] == "FAIL"]
    if failures:
        lines.extend(
            f"- `{row['generated_rule_id']}` `{row['case_type']}/{row['case_id']}`: returncode {row['returncode']}"
            for row in failures
        )
    else:
        lines.append("- None")
    lines.append("")
    (out / "core_run_execution_summary.md").write_text("\n".join(lines), encoding="utf-8")
