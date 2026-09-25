"""Pre/post execution evidence, not a conformance certificate or human approval."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import tempfile
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from .core_runner import (
    DEFAULT_TIMEOUT_SECONDS,
    CoreRunExecutionResult,
    CoreRunPlan,
    execute_core_run_plan,
)
from .errors import CliUsageError


def _file_digest(path: Path) -> dict[str, object]:
    if not path.is_file():
        raise CliUsageError(f"{path}: evidence requires a regular file")
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
            size += len(chunk)
    return {"sha256": digest.hexdigest(), "size_bytes": size}


def _input_snapshot(plan: CoreRunPlan) -> dict[str, object]:
    def listing_error(error: OSError) -> None:
        raise error

    paths: set[Path] = set()
    for item in plan.items:
        paths.add(Path(item.rule_yml))
        for name in ("manifest.json", "expected_results.csv"):
            path = Path(item.rule_dir) / name
            if path.exists():
                paths.add(path)
        data = Path(item.data_dir)
        if not data.is_dir():
            raise CliUsageError(f"{data}: case data directory is missing")
        # Unlike glob, traversal must not silently omit unreadable directories.
        for root, directories, files in os.walk(data, onerror=listing_error):
            for name in directories:
                path = Path(root) / name
                if path.is_symlink():
                    raise CliUsageError(f"{path}: linked data directories cannot be audited")
            paths.update(Path(root) / name for name in files)
    result = {}
    for path in sorted(paths):
        if not path.is_file():
            raise CliUsageError(f"{path}: audit input must be a readable regular file")
        result[str(path.absolute())] = _file_digest(path)
    return result


def _executable_snapshot(plan: CoreRunPlan, cwd: Path) -> dict[str, object]:
    result = {}
    search_path = os.pathsep.join(str((cwd / part).resolve()) for part in os.get_exec_path())
    for command in sorted({item.command[0] for item in plan.items}):
        resolved = (
            str((cwd / command).absolute())
            if os.path.dirname(command)
            else shutil.which(command, path=search_path)
        )
        if resolved and Path(resolved).is_file():
            result[command] = {"path": str(Path(resolved).resolve()), **_file_digest(Path(resolved))}
        else:
            result[command] = {"path": None, "sha256": None, "size_bytes": None}
    return result


def capture_run_snapshot(plan: CoreRunPlan, engine_cwd: Path) -> dict[str, object]:
    """Hash inputs and the invoked executable, not interpreter/build dependencies."""
    try:
        return {"inputs": _input_snapshot(plan), "executables": _executable_snapshot(plan, engine_cwd)}
    except OSError as error:
        raise CliUsageError(f"cannot capture run inputs: {error}") from error


def _write_completed_record(path: Path, record: dict[str, object]) -> None:
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as handle:
            temporary = Path(handle.name)
            json.dump(record, handle, indent=2, sort_keys=True)
            handle.write("\n")
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


@dataclass(frozen=True)
class RecordedCoreRun:
    execution: CoreRunExecutionResult
    integrity_ok: bool

    @property
    def ok(self) -> bool:
        return self.execution.ok and self.integrity_ok


def execute_recorded_core_run(
    plan: CoreRunPlan,
    evidence_path: Path,
    engine_cwd: str | Path | None = None,
    workers: int = 1,
    timeout_seconds: float | None = DEFAULT_TIMEOUT_SECONDS,
) -> RecordedCoreRun:
    """Keep a started record on interruption; finalize only after post-run hashing."""
    if plan.dry_run or any(item.dry_run for item in plan.items) or not plan.items:
        raise CliUsageError("execution evidence requires a non-empty, non-dry-run plan")
    output_dirs = [Path(item.output_dir) for item in plan.items]
    if len({path.resolve() for path in output_dirs}) != len(output_dirs):
        raise CliUsageError("each recorded case requires a distinct output directory")
    try:
        for path in output_dirs:
            path.mkdir(parents=True, exist_ok=False)
    except FileExistsError as error:
        raise CliUsageError(f"{path}: use new output directories for recorded cases") from error
    except OSError as error:
        raise CliUsageError(f"{path}: cannot reserve recorded case output: {error}") from error
    cwd = Path(engine_cwd or Path.cwd()).resolve()
    before = capture_run_snapshot(plan, cwd)
    record = {
        "schema_version": 1,
        "run_id": str(uuid.uuid4()),
        "state": "started",
        "started_at": datetime.now(timezone.utc).isoformat(),
        "finished_at": None,
        "approval": {"status": "not_reviewed"},
        "comparison_status": "not_performed",
        "engine_identity_scope": "invoked_executable_only",
        "settings": {"engine_cwd": str(cwd), "workers": workers, "timeout_seconds": timeout_seconds},
        "plan": plan.to_dict(),
        "before": before,
        "integrity_ok": None,
    }
    try:
        evidence_path.parent.mkdir(parents=True, exist_ok=True)
        with evidence_path.open("x", encoding="utf-8") as handle:
            json.dump(record, handle, indent=2, sort_keys=True)
            handle.write("\n")
    except OSError as error:
        raise CliUsageError(f"{evidence_path}: cannot create new run evidence: {error}") from error

    execution = execute_core_run_plan(plan, engine_cwd=cwd, workers=workers, timeout_seconds=timeout_seconds)
    errors = []
    after = None
    outputs = {}
    try:
        after = capture_run_snapshot(plan, cwd)
    except CliUsageError as error:
        errors.append(str(error))
    try:
        for item in plan.items:
            if not any((Path(item.output_dir) / name).is_file() for name in ("report.json", "report.csv")):
                errors.append(f"{item.output_dir}: no report was produced")
            for name in ("report.json", "report.csv", "validation.log"):
                path = Path(item.output_dir) / name
                if path.exists():
                    outputs[str(path.absolute())] = _file_digest(path)
    except (OSError, CliUsageError) as error:
        errors.append(f"cannot capture output: {error}")
    integrity_ok = after == before and not errors
    record.update({
        "state": "completed" if execution.ok and integrity_ok else "failed",
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "after": after,
        "integrity_ok": integrity_ok,
        "snapshot_errors": errors,
        "outputs": outputs,
        "execution": {"ok": execution.ok, "rows": execution.rows},
    })
    try:
        _write_completed_record(evidence_path, record)
    except OSError as error:
        raise CliUsageError(f"{evidence_path}: cannot finalize run evidence: {error}") from error
    return RecordedCoreRun(execution, integrity_ok)
