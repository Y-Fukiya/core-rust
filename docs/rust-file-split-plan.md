# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 23983 | Move Open Rules oracle-gate and loader tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 11818 | Continue extracting Open Rules compatibility helpers after the existing `open_rules_compat`, `standard_filter`, `usdm_jsonata`, and `condition_inspect` modules. |
| `crates/core-data/src/lib.rs` | 11264 | Extract Open Rules CSV/data-dir loading next to `open_rules_variables.rs`. |
| `crates/core-engine/src/lib.rs` | 4900 | Extract multi-row/group operator evaluation into operator modules. |
| `xtask/src/open_rules/score.rs` | 2823 | Split scoring, gate policy, and test fixtures once upstream baseline work settles. |

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
   `tests/open_rules_*.rs` modules. The first loader/row-scope slice now lives
   in `tests/open_rules_data_loader.rs`.
3. `core-data/src/lib.rs`: move `_datasets.csv` and Open Rules data-dir loader
   code beside `open_rules_variables.rs`.
4. `core-engine/src/lib.rs`: split group/relationship operator evaluators.
5. `xtask/src/open_rules/score.rs`: split `ScoreSummary`/`ScoreGate` policy
   from issue normalization.

## Completed Slices

- `core-api/src/open_rules_compat/classifier.rs`: oracle-gap classifier
  predicates and post-operator skip classification.
- `core-api/src/condition_inspect.rs`: pure condition tree inspection helpers
  used by Open Rules compatibility classification.
- `core-api/src/tests/open_rules_data_loader.rs`: first Open Rules data-loader
  and row-scope regression tests.

## Next Implementation Slice

The next low-risk code slice is:

- move another cohesive test family from `core-api/src/tests.rs` into
  `crates/core-api/src/tests/open_rules_*.rs`
- prefer tests that already build fixtures in memory and do not require
  production code changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_warns_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
