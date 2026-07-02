# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 18633 | Continue moving Open Rules fixture-style tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 11818 | Continue extracting Open Rules compatibility helpers after the existing `open_rules_compat`, `standard_filter`, `usdm_jsonata`, and `condition_inspect` modules. |
| `crates/core-data/src/lib.rs` | 10660 | Continue extracting USDM JSON flattening and dataset-package helpers after the Open Rules data-dir loader split. |
| `crates/core-engine/src/lib.rs` | 4632 | Continue extracting scalar/date/operator helper families after the group-operator split. |
| `xtask/src/open_rules/score.rs` | 2643 | Continue splitting issue normalization and scoring fixtures after the summary/gate/provenance policy split. |

## Principles

- Move one behavior family at a time, with no semantic changes in the same
  commit.
- Keep Open Rules oracle compatibility outside the production default path.
- Preserve the full upstream gate invariants:
  `supported_mismatch = 0`, `skipped_unsupported = 0`, `harness_error = 0`,
  and `deferred_oracle_gap_mismatch = 0`.
- Run targeted tests for the moved family before running workspace checks.

## Recommended Order

1. `core-api/src/lib.rs`: continue extracting small pure helper families into
   `open_rules_compat/` and sibling modules. The oracle-gap classifier and
   condition-inspection slices have already moved out of `lib.rs`.
2. `core-api/src/tests.rs`: continue moving Open Rules fixture-style tests into
   `tests/open_rules_*.rs` modules. Loader/row-scope and USDM slices have moved
   out already.
3. `core-data/src/lib.rs`: continue with USDM JSON flattening or dataset
   package helpers after the Open Rules CSV/data-dir loader split.
4. `core-engine/src/lib.rs`: continue splitting scalar/date/operator helper
   families after group/relationship operator evaluators moved out.
5. `xtask/src/open_rules/score.rs`: continue with issue normalization and
   test-fixture helpers after the `ScoreSummary`/`ScoreGate`/provenance
   policy split.

## Completed Slices

- `core-api/src/open_rules_compat/classifier.rs`: oracle-gap classifier
  predicates and post-operator skip classification.
- `core-api/src/condition_inspect.rs`: pure condition tree inspection helpers
  used by Open Rules compatibility classification.
- `core-api/src/tests/open_rules_data_loader.rs`: first Open Rules data-loader
  and row-scope regression tests.
- `core-api/src/tests/open_rules_usdm.rs`: USDM/Open Rules JSONata and USDM
  join regression tests.
- `core-api/src/tests/basic_validation.rs`: basic rule selection,
  preflight, and report-writing API tests.
- `core-data/src/open_rules_data_dir.rs`: Open Rules `_datasets.csv`,
  `_variables.csv`, embedded metadata, and CSV data-dir loading.
- `core-engine/src/group_operators.rs`: unique-set, relationship, and
  inconsistent-across-dataset operator evaluation.
- `xtask/src/open_rules/score/summary.rs`: scoreboard summary, deferred
  oracle-gap breakdown, group summaries, and score gate policy.
- `xtask/src/open_rules/score/provenance.rs`: candidate provenance parsing
  and detailed execution-provenance classification.

## Next Implementation Slice

The next low-risk code slice is:

- move another cohesive Open Rules test family from `core-api/src/tests.rs`,
  or split a pure helper family from `core-data/src/lib.rs`
- prefer code that already has focused tests and does not require behavior
  changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_fails_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
