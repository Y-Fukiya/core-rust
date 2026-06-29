from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Any, Iterable


NULL_STRINGS = {"", "nan", "none", "null", "na", "n/a"}
CSV_FORMULA_PREFIXES = ("=", "+", "-", "@")


def normalize_blank(value: object) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    if text.lower() in NULL_STRINGS:
        return None
    return text


def split_semicolon_list(value: object) -> list[str]:
    text = normalize_blank(value)
    if text is None:
        return []
    values = {part.strip() for part in text.split(";") if normalize_blank(part) is not None}
    return sorted(values)


def ensure_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                rows.append(json.loads(line))
    return rows


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    ensure_dir(path.parent)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False, sort_keys=True))
            handle.write("\n")


def csv_cell(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, (list, dict)):
        text = json.dumps(value, ensure_ascii=False, sort_keys=True)
    else:
        text = str(value)
    if text.lstrip().startswith(CSV_FORMULA_PREFIXES):
        return f"'{text}"
    return text


def write_csv(path: Path, rows: Iterable[dict[str, Any]], fieldnames: list[str]) -> None:
    ensure_dir(path.parent)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({key: csv_cell(row.get(key)) for key in fieldnames})
