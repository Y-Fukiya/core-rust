# Rulekit CORE Execution Closeout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the remaining Python rulekit tasks needed to execute generated Open Rules-style cases, compare actual CORE output structurally, export draft rules safely, and document the workflow.

**Architecture:** Keep Rust engine semantics untouched. Add adapters and orchestration in `src/cdisc_rulekit` only, treating generated Open Rules data directories as inputs and translating them to the existing `core-cli validate` surface.

**Tech Stack:** Python 3.11, pytest, subprocess, CSV/JSON/YAML files, existing `core-cli`.

---

### Task 1: CORE Run Execution

**Files:**
- Modify: `src/cdisc_rulekit/core_runner.py`
- Modify: `src/cdisc_rulekit/cli.py`
- Test: `tests/cdisc_rulekit/test_core_runner.py`
- Test: `tests/cdisc_rulekit/test_cli_run_compare.py`

- [ ] **Step 1: Write failing tests**

Add tests asserting that `build_core_run_plan()` passes dataset CSV files from each Open Rules `data/` directory and excludes `.env`, `_datasets.csv`, and `_variables.csv`. Add a CLI test that runs `run-core` without `--dry-run` against a tiny fake engine command and verifies execution summary output.

- [ ] **Step 2: Verify tests fail**

Run: `python -m pytest tests/cdisc_rulekit/test_core_runner.py tests/cdisc_rulekit/test_cli_run_compare.py -q`
Expected: FAIL because non-dry-run execution and CSV-file adapter are missing.

- [ ] **Step 3: Implement minimal code**

Add a dataset-file discovery helper, a `CoreRunExecutionResult`, and an `execute_core_run_plan()` function using `subprocess.run()`. Update `cmd_run_core()` to write the plan, execute when not dry-run, write JSON/CSV/Markdown execution summaries, and return nonzero when any case fails.

- [ ] **Step 4: Verify tests pass**

Run: `python -m pytest tests/cdisc_rulekit/test_core_runner.py tests/cdisc_rulekit/test_cli_run_compare.py -q`
Expected: PASS.

### Task 2: Actual Report Comparison Robustness

**Files:**
- Modify: `src/cdisc_rulekit/compare_results.py`
- Test: `tests/cdisc_rulekit/test_compare_results.py`

- [ ] **Step 1: Write failing tests**

Add tests for real `core-report` JSON shape where `errors` carries `usubjid` and `seq`, and for CSV shape with `execution_status=skipped` to ensure skipped is tracked separately from failed mismatches.

- [ ] **Step 2: Verify tests fail**

Run: `python -m pytest tests/cdisc_rulekit/test_compare_results.py -q`
Expected: FAIL because comparison rows do not yet expose skipped counts or optional `usubjid`/`seq` matching.

- [ ] **Step 3: Implement minimal code**

Parse `usubjid`, `seq`, and skipped report rows, add status values that distinguish `ACTUAL_SKIPPED` from `FAIL`, and keep diagnostic message text out of the matching key.

- [ ] **Step 4: Verify tests pass**

Run: `python -m pytest tests/cdisc_rulekit/test_compare_results.py -q`
Expected: PASS.

### Task 3: Generator Type Authority And Same-Record Conditions

**Files:**
- Modify: `src/cdisc_rulekit/generate_rules.py`
- Test: `tests/cdisc_rulekit/test_generate_rules.py`

- [ ] **Step 1: Write failing tests**

Add tests showing `_variables.csv` numeric type selection for numeric-looking variables and a simple same-record condition rule that creates two checks in `Check.all`.

- [ ] **Step 2: Verify tests fail**

Run: `python -m pytest tests/cdisc_rulekit/test_generate_rules.py -q`
Expected: FAIL because variables are always `Char` and simple condition generation is missing.

- [ ] **Step 3: Implement minimal code**

Infer numeric type for numeric generated values and add a conservative parser for raw condition keys already normalized by P21 loading, only for same-record `when`/`then` equality or non-empty checks.

- [ ] **Step 4: Verify tests pass**

Run: `python -m pytest tests/cdisc_rulekit/test_generate_rules.py -q`
Expected: PASS.

### Task 4: Export Command And Documentation

**Files:**
- Create: `src/cdisc_rulekit/export_rules.py`
- Modify: `src/cdisc_rulekit/cli.py`
- Modify: `README.md`
- Test: `tests/cdisc_rulekit/test_export_rules.py`

- [ ] **Step 1: Write failing tests**

Add tests that export generated rules into `Unpublished/NEW-RULE/<rule_id>/` without overwriting existing directories unless `--overwrite` is passed.

- [ ] **Step 2: Verify tests fail**

Run: `python -m pytest tests/cdisc_rulekit/test_export_rules.py -q`
Expected: FAIL because `export-rules` does not exist.

- [ ] **Step 3: Implement minimal code**

Copy generated rule directories with overwrite protection, emit an export manifest, and wire `export-rules` into the CLI.

- [ ] **Step 4: Verify tests pass and run full Python suite**

Run: `python -m pytest tests/cdisc_rulekit -q`
Expected: PASS.

### Task 5: Pilot And Final Verification

**Files:**
- Generated output under `output/`

- [ ] **Step 1: Re-run generated structure validation**

Run: `PYTHONPATH=src python -m cdisc_rulekit.cli validate-structure --generated-rules output/sdtmig_phase2/generated_rules --out output/sdtmig_phase2/reports`
Expected: OK.

- [ ] **Step 2: Re-run CORE dry-run**

Run: `PYTHONPATH=src python -m cdisc_rulekit.cli run-core --generated-rules output/sdtmig_phase2/generated_rules --out output/sdtmig_phase2 --dry-run`
Expected: 34 cases planned with dataset CSV file arguments.

- [ ] **Step 3: Run final tests**

Run: `python -m pytest tests/cdisc_rulekit -q`
Expected: PASS.
