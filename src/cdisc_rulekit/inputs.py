from __future__ import annotations

import os
import shutil
import zipfile
from pathlib import Path


def looks_like_open_rules_repo(path: str | Path) -> bool:
    root = Path(path)
    return (root / "Published").is_dir() or (root / "Unpublished").is_dir()


def locate_open_rules_repo(path: str | Path) -> Path:
    root = Path(path)
    if looks_like_open_rules_repo(root):
        return root
    if not root.is_dir():
        return root

    candidates = [child for child in sorted(root.iterdir()) if child.is_dir() and looks_like_open_rules_repo(child)]
    if not candidates:
        return root
    return max(candidates, key=lambda candidate: len(list((candidate / "Published").rglob("rule.yml"))))


def _assert_safe_zip_member(member_name: str, target: Path) -> None:
    member_path = Path(member_name)
    if member_path.is_absolute() or ".." in member_path.parts:
        raise ValueError(f"unsafe zip member path: {member_name}")
    destination = (target / member_path).resolve()
    target_root = target.resolve()
    if os.path.commonpath([str(target_root), str(destination)]) != str(target_root):
        raise ValueError(f"unsafe zip member path: {member_name}")


def extract_open_rules_zip(archive_path: str | Path, work_root: str | Path) -> Path:
    archive = Path(archive_path)
    target = Path(work_root) / "open_rules_zip"
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(archive) as zip_handle:
        for member in zip_handle.infolist():
            _assert_safe_zip_member(member.filename, target)
        zip_handle.extractall(target)

    return locate_open_rules_repo(target)


def resolve_open_rules_input(open_rules_path: str | Path, work_root: str | Path) -> Path:
    path = Path(open_rules_path)
    if path.suffix.lower() == ".zip":
        return extract_open_rules_zip(path, work_root)
    return locate_open_rules_repo(path)
