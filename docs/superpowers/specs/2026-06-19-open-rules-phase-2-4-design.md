# Open Rules Phase 2-4 Design

Date: 2026-06-19
Status: approved design

## Purpose

Continue the CDISC Open Rules oracle work beyond the Phase 1 read-only scorer.
The next milestone is to make `core-rust` produce candidate reports for Open
Rules cases, score those reports against official `results.csv` files, and keep
that process stable in CI.

This design covers:

- Phase 2: `_variables.csv` schema-aware CSV loading.
- Phase 3: core-rust execution harness for Open Rules cases.
- Phase 4: baseline policy, strict upstream lock checks, and CI wiring.

The work must preserve the Phase 1 separation between skipped coverage gaps and
supported mismatches. It must not compare diagnostic message text as a primary
correctness key.

## Chosen Approach

Implement Phase 2-4 on the existing `codex/open-rules-oracle-harness` branch as
one continuous change set.

Use `core-data` for Open Rules data loading, `core-api` for validation, and
`xtask` for corpus orchestration. The runner should call the Rust API directly
rather than spawning the `core-rs` CLI. This keeps the harness deterministic,
fast, and easy to unit-test while leaving the user-facing CLI behavior intact.

## Scope

In scope:

- Add an Open Rules-specific data directory loader that reads `_datasets.csv`
  and `_variables.csv`.
- Preserve the existing generic CSV loader and its inference behavior.
- Extend `ValidateRequest` so callers can explicitly choose the Open Rules data
  loader.
- Add `xtask open-rules run` to generate candidate reports in the mirrored
  Phase 1 layout.
- Add a convenient combined path that runs cases and then scores the generated
  reports.
- Add a baseline artifact and a command path that compares a current scoreboard
  against that baseline.
- Add strict upstream lock enforcement as an opt-in flag.
- Add CI jobs that verify Rust code and the repository-local Open Rules fixture.
- Update user-facing documentation.

Out of scope:

- Engine semantic fixes for mismatching rules.
- Downloading or vendoring the full `cdisc-open-rules` repository in CI.
- Making the real upstream corpus a required PR check.
- LLM-generated augmentation data.
- Replacing the public `core-rs validate` CLI with Open Rules-specific behavior.

## Architecture

### `core-data`

Add a dedicated Open Rules data path, for example:

```rust
pub fn load_open_rules_data_dir(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>>
```

The loader reads:

- `_datasets.csv`: dataset identity, file names, and labels.
- `_variables.csv`: dataset variables, labels, declared type, and length.
- Dataset CSV files referenced by `_datasets.csv`.

The existing `load_csv_dataset` and `load_datasets_from_paths` behavior remains
unchanged. Generic CSV input keeps using the current inference path.

### `core-api`

Extend `ValidateRequest` with a small loader selection enum:

```rust
pub enum DatasetLoader {
    Generic,
    OpenRulesDataDir,
}
```

The default stays `Generic`, preserving existing tests and CLI behavior. The
Open Rules harness sets `OpenRulesDataDir` when validating case `data/`
directories.

### `xtask`

Keep the Phase 1 modules and add focused orchestration modules:

- `run.rs`: execute discovered Open Rules cases through `core-api`.
- `baseline.rs`: compare scoreboard summaries and case buckets against an
  accepted baseline.
- `strict_lock` support in `upstream.rs` or command-level validation.

Expose these commands:

```sh
cargo run -p xtask -- open-rules run \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs

cargo run -p xtask -- open-rules run-score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard

cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

`score` remains available and keeps its Phase 1 behavior.

## Data Loading Semantics

The Open Rules data loader treats `_variables.csv` as the schema authority.

Rules:

- Match variable metadata to dataset files by dataset/domain name
  case-insensitively.
- Preserve CSV column order from the dataset file.
- Preserve declared variable order in metadata.
- Use declared type for value conversion:
  - `Char`, `Character`, `Text`, `String` -> string values.
  - `Num`, `Numeric`, `Integer`, `Float`, `Double` -> numeric values when
    parseable.
- Empty cells become null for numeric variables and empty strings for character
  variables.
- Values that cannot be parsed as declared numeric type become null and produce
  a load warning, not a panic.
- Variables declared in `_variables.csv` but missing from the CSV are retained
  in metadata and reported as warnings.
- CSV columns missing from `_variables.csv` are loaded as strings and reported
  as warnings.
- Dataset labels and variable labels/lengths are stored in `DatasetMetadata`.

This deliberately avoids applying Open Rules schema behavior to normal CSV
validation input.

## Execution Harness

`xtask open-rules run` discovers cases using the Phase 1 discovery module. For
each case it calls `core_api::run_validation` with:

- `rule_paths`: the case rule's `rule.yml`.
- `dataset_paths`: the case `data/` directory.
- `dataset_loader`: `OpenRulesDataDir`.
- `include_rules`: the discovered `rule_id`, so each case runs only its rule.
- `output_format`: CSV or both, as needed by scoring.
- `output_dir`: mirrored report directory.

The mirrored candidate layout remains:

```text
<core-rs-results-root>/<scope>/<rule_id>/<case_kind>/<case_id>/report.csv
```

Per-case execution errors should not abort the whole run. Instead, the runner
writes a minimal skipped or harness-error marker that the scorer can classify
deterministically, and it records the error in a run summary.

`run-score` runs the same execution step and then invokes the existing scorer
against the generated candidate report root.

## Baseline Policy

Add `tests/open_rules/baseline.json` as the accepted state for the
repository-local Open Rules fixture.

The baseline records:

- upstream expected SHA used for the run,
- total summary counts,
- per-case bucket,
- optional reason for skipped or harness-error cases.

Baseline comparison rules:

- New `supported_mismatch` cases fail.
- New `harness_error` cases fail.
- Regressions from `supported_match` to any other bucket fail.
- Improvements from `skipped_unsupported` or `supported_mismatch` to
  `supported_match` are allowed but must be visible in the diff output.
- Upstream SHA mismatch fails only when `--strict-lock` is set.

This keeps CI useful before full upstream coverage is realistic.

## CI

Update `.github/workflows/ci.yml` to keep existing Rust checks and add a
fixture-level Open Rules oracle check:

```sh
cargo run -p xtask -- open-rules run-score \
  --open-rules-root tests/fixtures/open_rules_executable \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard-fixture

cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard-fixture/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

The full upstream `cdisc-open-rules` checkout remains a local/manual or future
scheduled workflow because network access and upstream drift should not block
ordinary pull requests.

## Error Handling

Use `anyhow` at the `xtask` command boundary and typed errors inside reusable
library code.

Failure categories:

- Schema metadata missing or malformed: harness error.
- Dataset CSV missing: harness error.
- Per-row numeric parse issue: warning with null fallback.
- Unsupported rule or engine capability: skipped unsupported.
- Candidate and official structural issue set differs: supported mismatch.

The scorer remains the authority for final bucket assignment.

## Testing

Tests must not require a real `cdisc-open-rules` checkout.

Add a new executable Open Rules fixture under:

```text
tests/fixtures/open_rules_executable
```

It should include at least:

- A passing positive case.
- A failing negative case that produces a deterministic issue key.
- A numeric string case where `_variables.csv` prevents accidental string
  comparison behavior.
- A case that exercises missing metadata warnings without crashing.

Required tests:

- `core-data` loads Open Rules data dirs with schema-aware types.
- Existing generic CSV loading behavior is unchanged.
- `core-api` uses the requested loader mode.
- `xtask open-rules run` writes mirrored candidate reports.
- `run-score` produces a scoreboard from generated reports.
- Baseline comparison fails on regressions and passes on the accepted fixture.
- Strict lock mismatch fails only with `--strict-lock`.
- Existing workspace tests, fmt, clippy, and fixture run-score all pass.

## Documentation

Update `docs/open-rules-oracle-harness.md` with:

- Phase 2 schema-aware loader behavior.
- Phase 3 execution commands.
- Phase 4 baseline and CI behavior.
- Guidance for running against a real local `cdisc-open-rules` checkout.
- Reminder that upstream SHA bumps should be reviewed in their own PR.

## Success Criteria

Phase 2-4 are complete when:

- `core-rust` can load Open Rules case data with `_variables.csv` type
  authority.
- `xtask open-rules run-score` can generate candidate reports and score them.
- A repository-local executable fixture proves the full run -> score ->
  baseline flow.
- CI runs that fixture flow without network dependency.
- Strict upstream lock enforcement exists for local/full-corpus runs.
- Documentation explains the operational process clearly.
