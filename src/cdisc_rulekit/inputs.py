from __future__ import annotations

import os
import shutil
import zipfile
from pathlib import Path

from .errors import CliUsageError


def looks_like_open_rules_repo(path: str | Path) -> bool:
    root = Path(path)
    return (root / "Published").is_dir() or (root / "Unpublished").is_dir()


def require_legacy_open_rules_root(root: Path) -> None:
    if not root.is_dir():
        raise CliUsageError(f"Open Rules input is not a directory: {root}")
    if (root / "sdtm_rules").exists():
        raise CliUsageError(
            f"unsupported Open Rules format at {root}: sdtm_rules (CORE v1 contributor), "
            "including mixed legacy/v1 roots, is not supported; expected Published/Unpublished rule.yml layout"
        )
    if not looks_like_open_rules_repo(root):
        raise CliUsageError(f"unsupported Open Rules format at {root}: expected Published/Unpublished rule.yml layout")


def locate_open_rules_repo(path: str | Path) -> Path:
    root = Path(path)
    if looks_like_open_rules_repo(root) or (root / "sdtm_rules").exists():
        require_legacy_open_rules_root(root)
        return root
    if not root.is_dir():
        raise CliUsageError(f"Open Rules input is not a directory: {root}")

    candidates = [
        child for child in sorted(root.iterdir())
        if child.is_dir() and (looks_like_open_rules_repo(child) or (child / "sdtm_rules").exists())
    ]
    if len(candidates) > 1:
        raise CliUsageError(
            "ambiguous Open Rules input; supply an exact package root: "
            + ", ".join(str(candidate) for candidate in candidates)
        )
    selected = candidates[0] if candidates else root
    require_legacy_open_rules_root(selected)
    return selected


def _assert_safe_zip_member(member_name: str, target: Path) -> None:
    member_path = Path(member_name)
    if member_path.is_absolute() or ".." in member_path.parts:
        raise CliUsageError(f"unsafe zip member path: {member_name}")
    destination = (target / member_path).resolve()
    target_root = target.resolve()
    if os.path.commonpath([str(target_root), str(destination)]) != str(target_root):
        raise CliUsageError(f"unsafe zip member path: {member_name}")


def _is_cache_zip_member(member_name: str) -> bool:
    parts = Path(member_name).parts
    return any(
        part in {"__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".DS_Store"}
        or part.endswith((".pyc", ".pyo"))
        for part in parts
    )


def extract_open_rules_zip(archive_path: str | Path, work_root: str | Path) -> Path:
    archive = Path(archive_path)
    target = Path(work_root) / "open_rules_zip"
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(archive) as zip_handle:
        for member in zip_handle.infolist():
            _assert_safe_zip_member(member.filename, target)
            if _is_cache_zip_member(member.filename):
                continue
            zip_handle.extract(member, target)

    return locate_open_rules_repo(target)


def resolve_open_rules_input(open_rules_path: str | Path, work_root: str | Path) -> Path:
    path = Path(open_rules_path)
    if path.suffix.lower() == ".zip":
        return extract_open_rules_zip(path, work_root)
    return locate_open_rules_repo(path)
