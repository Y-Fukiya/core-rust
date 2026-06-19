# P21 Open Rules Read-Only Toolkit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build a read-only Python companion toolkit that ingests P21 rule extracts and local CDISC Open Rules files, then emits normalized catalogs, mappings, conversion status, and reports under `output/`.

**Architecture:** Add a small Python package under `src/cdisc_rulekit` with focused modules for models, I/O, P21 loading, Open Rules loading, operator inventory, mapping, classification, reports, and CLI orchestration. Keep Rust code untouched and connect to the existing repository only through generated artifacts. Follow TDD for production code: each feature starts with a failing pytest, then minimal implementation.

**Tech Stack:** Python 3.11+, standard library `csv/json/argparse/dataclasses/pathlib/difflib`, `PyYAML`, `pytest`.

---

## File Structure

- Create `pyproject.toml`: Python package metadata and pytest path config.
- Create `src/cdisc_rulekit/__init__.py`: package marker and version.
- Create `src/cdisc_rulekit/models.py`: dataclasses and serialization helpers.
- Create `src/cdisc_rulekit/io_utils.py`: CSV/JSONL readers and writers, blank/list normalization, safe path creation.
- Create `src/cdisc_rulekit/load_p21.py`: P21 CSV loading, domain-map join, CG ID extraction.
- Create `src/cdisc_rulekit/load_open_rules.py`: Open Rules YAML loading and test data inventory.
- Create `src/cdisc_rulekit/operator_inventory.py`: recursive `Check` tree operator/shape inventory.
- Create `src/cdisc_rulekit/map_rules.py`: CG ID and conservative fuzzy mapping.
- Create `src/cdisc_rulekit/classify.py`: conversion status and reason-code assignment.
- Create `src/cdisc_rulekit/emit.py`: artifact emission helpers.
- Create `src/cdisc_rulekit/reports.py`: Markdown and JSON readiness summaries.
- Create `src/cdisc_rulekit/cli.py`: `argparse` CLI commands.
- Create `tests/cdisc_rulekit/fixtures/...`: minimal P21 and Open Rules fixtures.
- Create `tests/cdisc_rulekit/test_*.py`: pytest coverage for each module and the read-only CLI.

## Task 1: Package Skeleton, Models, I/O, And Fixtures

**Files:**
- Create: `pyproject.toml`
- Create: `src/cdisc_rulekit/__init__.py`
- Create: `src/cdisc_rulekit/models.py`
- Create: `src/cdisc_rulekit/io_utils.py`
- Create: `tests/cdisc_rulekit/conftest.py`
- Create: `tests/cdisc_rulekit/fixtures/p21/cdisc_rule_definitions_latest_2204.csv`
- Create: `tests/cdisc_rulekit/fixtures/p21/cdisc_rule_domain_map.csv`
- Create: `tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/rule.yml`
- Create: `tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/_datasets.csv`
- Create: `tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/_variables.csv`
- Create: `tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/ae.csv`
- Create: `tests/cdisc_rulekit/fixtures/open_rules/Unpublished/CORE-DRAFT-0001/rule.yml`
- Create: `tests/cdisc_rulekit/test_models_io.py`

- [x] **Step 1: Write failing model and I/O tests**

Create `tests/cdisc_rulekit/test_models_io.py` with tests that import `CanonicalRule`, `RuleMapping`, `OperatorInventoryItem`, `normalize_blank`, `split_semicolon_list`, `write_jsonl`, and `read_jsonl`.

Assertions:
- `normalize_blank("")`, `normalize_blank(" nan ")`, and `normalize_blank(None)` return `None`.
- `split_semicolon_list(" AE ; CM;AE ")` returns `["AE", "CM"]`.
- `CanonicalRule(...).to_dict()` serializes list and dict fields.
- JSONL round trip preserves two rows exactly.

- [x] **Step 2: Run RED test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_models_io.py -q
```

Expected: FAIL with `ModuleNotFoundError: No module named 'cdisc_rulekit'`.

- [x] **Step 3: Implement package skeleton and helpers**

Create `pyproject.toml` with a setuptools package using `src` layout, Python `>=3.11`, dependencies `PyYAML`, optional test dependency `pytest`, and pytest config `pythonpath = ["src"]`.

Implement dataclasses:

- `CanonicalRule`
- `RuleMapping`
- `OperatorInventoryItem`

Each dataclass gets `to_dict()`. `CanonicalRule.from_dict()` and `RuleMapping.from_dict()` should normalize list/dict fields loaded from JSONL.

Implement `io_utils.py` helpers:

- `normalize_blank(value: object) -> str | None`
- `split_semicolon_list(value: object) -> list[str]`
- `ensure_dir(path: Path) -> None`
- `read_jsonl(path: Path) -> list[dict[str, object]]`
- `write_jsonl(path: Path, rows: Iterable[dict[str, object]]) -> None`
- `write_csv(path: Path, rows: Iterable[dict[str, object]], fieldnames: list[str]) -> None`

- [x] **Step 4: Add minimal fixtures**

Create P21 fixture CSV with three rules:

- `SD0001` with `CG0001`, SDTM-IG, Match, AE, variable `AETERM`.
- `SD0002` with no CG ID, SDTM-IG, Regex, AE, variable `AEDTC`.
- `DEF001` with Define.xml/Schematron style metadata requiring manual review.

Create domain map fixture with active AE mapping for `SD0001` and an inactive CM mapping that should not be joined.

Create Open Rules fixture `CORE-000001/rule.yml` referencing `CG0001`, domain AE, variable `AETERM`, and a simple `Check` tree. Create one positive test-data directory with `_datasets.csv`, `_variables.csv`, and `ae.csv`.

Create `Unpublished/CORE-DRAFT-0001/rule.yml` referencing `CG9999` so default scanning can prove it is excluded.

- [x] **Step 5: Run GREEN test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_models_io.py -q
```

Expected: PASS.

## Task 2: P21 Loader

**Files:**
- Create: `src/cdisc_rulekit/load_p21.py`
- Create: `tests/cdisc_rulekit/test_load_p21.py`

- [x] **Step 1: Write failing P21 loader tests**

Create tests for `load_p21_rules(rules_path, domain_map_path=None)`:

- Loading fixture returns three `CanonicalRule` rows.
- `SD0001` has `source == "P21"`, `p21_rule_id == "SD0001"`, `cdisc_rule_ids == ["CG0001"]`, `domains == ["AE"]`, `classes == ["EVENTS"]`, and `variables == ["AETERM"]`.
- Blank and `nan` fields become `None` or empty lists.
- Active domain map rows are joined, inactive rows are excluded.
- `DEF001` keeps raw XML path and can later classify as manual.

- [x] **Step 2: Run RED test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_load_p21.py -q
```

Expected: FAIL with `ImportError` for missing `load_p21`.

- [x] **Step 3: Implement P21 loader**

Implement:

- `CG_RE = re.compile(r"\bCG\d{4,}\b")`
- `extract_cg_ids(*values: object) -> list[str]`
- `load_p21_rules(rules_path: Path, domain_map_path: Path | None = None) -> tuple[list[CanonicalRule], list[str]]`
- Active domain-map indexing by `config_version`, `agency`, `config_name`, `standard_name`, `standard_version`, `rule_id`, and `source_xml_path`.
- Raw condition dict containing `target`, `variable`, `when`, `if`, `test`, `where`, `search`, `from`, `terms`, `group_by`, `matching`, `optional`, `ignore_context`, `match_exact`, and JSON fields when present.

Do not discard unknown columns; preserve every row in `raw_record`.

- [x] **Step 4: Run GREEN test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_load_p21.py tests/cdisc_rulekit/test_models_io.py -q
```

Expected: PASS.

## Task 3: Open Rules Loader And Operator Inventory

**Files:**
- Create: `src/cdisc_rulekit/load_open_rules.py`
- Create: `src/cdisc_rulekit/operator_inventory.py`
- Create: `tests/cdisc_rulekit/test_load_open_rules.py`
- Create: `tests/cdisc_rulekit/test_operator_inventory.py`

- [x] **Step 1: Write failing Open Rules loader tests**

Create tests for `load_open_rules(repo_path, include_unpublished=False)`:

- Default scan returns only `CORE-000001`.
- `include_unpublished=True` returns `CORE-000001` and `CORE-DRAFT-0001`.
- `CORE-000001` has `core_rule_id == "CORE-000001"`, `cdisc_rule_ids == ["CG0001"]`, `domains == ["AE"]`, `variables` containing `AETERM`, and a message.
- Test-data inventory includes `Published`, `CORE-000001`, `positive`, `01`, and `ae.csv`.

- [x] **Step 2: Write failing operator inventory tests**

Create tests for `build_operator_inventory(core_rules)`:

- Inventory includes at least one item for `CORE-000001`.
- At least one item has `operator` equal to an observed `Check` key from the fixture.
- `raw_keys` records sorted dictionary keys.

- [x] **Step 3: Run RED tests**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_load_open_rules.py tests/cdisc_rulekit/test_operator_inventory.py -q
```

Expected: FAIL with missing modules.

- [x] **Step 4: Implement Open Rules loader**

Implement safe YAML parsing with `yaml.safe_load`. If `PyYAML` import fails, raise a clear runtime error naming `PyYAML`.

Implement:

- `discover_rule_yml(repo_path, include_unpublished=False) -> list[Path]`
- `load_open_rules(repo_path: Path, include_unpublished: bool = False) -> tuple[list[CanonicalRule], list[dict[str, object]], list[str]]`
- recursive helpers for nested get, authority extraction, CG fallback extraction from raw YAML text, and `name` collection under `Check`.
- `inventory_testdata(rule_dir: Path) -> list[dict[str, object]]`

Malformed YAML should append a warning and continue.

- [x] **Step 5: Implement operator inventory**

Implement `build_operator_inventory(core_rules: list[CanonicalRule]) -> list[OperatorInventoryItem]`.

The function recursively walks `rule.raw_condition["Check"]`. For dictionary nodes, emit one item for each operator-like key. Exclude metadata-like keys: `name`, `value`, `values`, `variable`, `variables`, `dataset`, `domain`, `message`, `description`, `label`, `type`, `length`.

- [x] **Step 6: Run GREEN tests**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_load_open_rules.py tests/cdisc_rulekit/test_operator_inventory.py -q
```

Expected: PASS.

## Task 4: Mapping And Classification

**Files:**
- Create: `src/cdisc_rulekit/map_rules.py`
- Create: `src/cdisc_rulekit/classify.py`
- Create: `tests/cdisc_rulekit/test_map_rules.py`
- Create: `tests/cdisc_rulekit/test_classify.py`

- [x] **Step 1: Write failing mapping tests**

Create tests for `map_p21_to_core(p21_rules, core_rules)`:

- `SD0001` maps to `CORE-000001` with `match_type == "CG_ID"` and `confidence >= 0.90`.
- A P21 rule with no CG but matching standard/domain/variable/message emits `FUZZY` and does not use `CG_ID`.
- An unrelated P21 rule emits `NONE` with confidence `0`.
- The function returns exactly one mapping per P21 rule.

- [x] **Step 2: Write failing classification tests**

Create tests for `classify_rules(p21_rules, mappings)`:

- CG ID mapped `SD0001` becomes `NATIVE_CORE` with `HAS_NATIVE_CORE_MAPPING`.
- Simple Regex `SD0002` becomes `AUTO_CONVERTIBLE` with `SIMPLE_REGEX`.
- Define.xml/Schematron row becomes `MANUAL_REQUIRED` with `DEFINE_XML_RULE` or `SCHEMATRON_RULE`.
- A malformed synthetic row with no rule id becomes `UNSUPPORTED`.
- Fuzzy mapping never becomes `NATIVE_CORE`.

- [x] **Step 3: Run RED tests**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_map_rules.py tests/cdisc_rulekit/test_classify.py -q
```

Expected: FAIL with missing modules.

- [x] **Step 4: Implement mapping**

Implement:

- `standard_key(value: str | None) -> str`
- `token_similarity(left: str | None, right: str | None) -> float`
- `overlap(left: list[str], right: list[str]) -> list[str]`
- `map_p21_to_core(p21_rules, core_rules) -> list[RuleMapping]`

Mapping priority:

1. Shared CG IDs: `CG_ID`, base `0.90`, increments for standard/domain/variable overlap, cap `1.0`.
2. Fuzzy: score standard, domain, variable, message, and description. Emit `FUZZY` only at `>= 0.60`.
3. None: `NONE`, confidence `0`.

- [x] **Step 5: Implement classification**

Implement:

- `classify_rules(p21_rules: list[CanonicalRule], mappings: list[RuleMapping]) -> list[CanonicalRule]`
- conservative helpers for Define.xml, Schematron, metadata, cross-domain, lookup, unresolved macro, concrete domain, target variable, and simple rule types.

Set `conversion_status`, `conversion_reasons`, `conversion_confidence`, and `core_rule_id` on returned copies. Never mutate caller-owned rules in place.

- [x] **Step 6: Run GREEN tests**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_map_rules.py tests/cdisc_rulekit/test_classify.py -q
```

Expected: PASS.

## Task 5: Emitters, Reports, And CLI

**Files:**
- Create: `src/cdisc_rulekit/emit.py`
- Create: `src/cdisc_rulekit/reports.py`
- Create: `src/cdisc_rulekit/cli.py`
- Create: `tests/cdisc_rulekit/test_cli_readonly.py`

- [x] **Step 1: Write failing CLI test**

Create a test that runs:

```python
subprocess.run([
    sys.executable, "-m", "cdisc_rulekit.cli", "build-readonly",
    "--p21-rules", str(p21_rules),
    "--p21-domain-map", str(p21_domain_map),
    "--open-rules-repo", str(open_rules_repo),
    "--out", str(tmp_path / "output"),
    "--standard", "SDTM-IG",
], check=True, capture_output=True, text=True)
```

Assert these files exist:

- `catalog/p21_rules_normalized.csv`
- `catalog/p21_rules_normalized.jsonl`
- `catalog/core_rules_normalized.csv`
- `catalog/core_rules_normalized.jsonl`
- `catalog/core_testdata_inventory.csv`
- `catalog/core_operator_inventory.csv`
- `catalog/core_operator_inventory.jsonl`
- `catalog/conversion_status.csv`
- `mapping/p21_to_core_mapping.csv`
- `mapping/p21_to_core_mapping.jsonl`
- `reports/conversion_status_summary.md`
- `reports/readiness_summary.json`

Assert `generated_rules` does not exist.

- [x] **Step 2: Run RED test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_cli_readonly.py -q
```

Expected: FAIL with missing CLI module or missing outputs.

- [x] **Step 3: Implement emitters and reports**

Implement:

- `emit_p21_catalog(out_dir, rules)`
- `emit_core_catalog(out_dir, rules, testdata_inventory, operator_inventory)`
- `emit_mapping(out_dir, mappings)`
- `emit_conversion_status(out_dir, classified_rules)`
- `write_conversion_summary(report_dir, classified_rules, warnings)`
- `write_readiness_summary(report_dir, classified_rules, mappings, warnings)`

Use deterministic fieldname order. Lists and dictionaries in CSV cells are JSON strings.

- [x] **Step 4: Implement CLI**

Use `argparse` subcommands:

- `ingest-p21`
- `ingest-open-rules`
- `map`
- `classify`
- `build-readonly`

`build-readonly` runs the full read-only pipeline. It accepts `--standard` and `--limit`; these filter output candidates before classification only when provided, without changing loader behavior.

No `generate`, `validate-structure`, or export commands are exposed.

- [x] **Step 5: Run GREEN CLI test**

Run:

```sh
python -m pytest tests/cdisc_rulekit/test_cli_readonly.py -q
```

Expected: PASS.

## Task 6: Full Verification And Documentation Touch-Up

**Files:**
- Modify: `README.md` only if a short read-only toolkit section is needed.
- Modify: `docs/superpowers/plans/2026-06-19-p21-open-rules-readonly-toolkit.md` to check completed boxes if executing manually.

- [x] **Step 1: Run all Python tests**

Run:

```sh
python -m pytest tests/cdisc_rulekit -q
```

Expected: PASS.

- [x] **Step 2: Run focused Rust tests only if touched**

Because this plan should not modify Rust code, do not run full Rust verification for the Python toolkit unless Rust files were touched. If Rust files are touched accidentally, stop and inspect why.

- [x] **Step 3: Inspect git status**

Run:

```sh
git status --short
```

Expected: only Python toolkit files, test fixtures, `pyproject.toml`, and this plan are staged or modified by this work. Pre-existing unrelated Rust and duplicate files may remain unstaged.

- [x] **Step 4: Commit only this toolkit work**

Run:

```sh
git add pyproject.toml src/cdisc_rulekit tests/cdisc_rulekit docs/superpowers/plans/2026-06-19-p21-open-rules-readonly-toolkit.md
git commit -m "feat: add read-only P21 Open Rules toolkit"
```

Expected: commit succeeds without staging unrelated Rust changes.

## Self-Review

- Spec coverage: The plan covers package setup, P21 ingest, Open Rules ingest, operator inventory, mapping, classification, reports, CLI, and fixture-based tests.
- Read-only boundary: The plan does not create generated rules, write positive or negative generated data, modify `Published`, or change Rust semantics.
- TDD: Each production module has failing tests before implementation.
- Artifact contract: Outputs match the approved design.
- Ambiguity check: `TEST_DATA_ONLY` is reserved; `NATIVE_CORE` takes precedence for CG ID matches; fuzzy mapping cannot become native coverage.
