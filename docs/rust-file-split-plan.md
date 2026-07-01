# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 23867 | Move Open Rules oracle-gate tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 12363 | Extract oracle-gap classification helpers after the existing `open_rules_compat`, `standard_filter`, and `usdm_jsonata` modules. |
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

1. `core-api/src/lib.rs`: extract oracle-gap classifier predicates into
   `open_rules_compat/classifier.rs`. This removes the densest rule-policy
   block without touching engine execution.
2. `core-api/src/tests.rs`: move oracle-gap manifest and score-gate tests into
   `tests/open_rules_oracle_gap.rs` style modules.
3. `core-data/src/lib.rs`: move `_datasets.csv` and Open Rules data-dir loader
   code beside `open_rules_variables.rs`.
4. `core-engine/src/lib.rs`: split group/relationship operator evaluators.
5. `xtask/src/open_rules/score.rs`: split `ScoreSummary`/`ScoreGate` policy
   from issue normalization.

## First Implementation Slice

The next low-risk code slice is:

- add `crates/core-api/src/open_rules_compat/classifier.rs`
- move only pure predicates that depend on
  `rule_id_has_oracle_gap_category`, `ExecutableRule`, `RuleType`,
  `Sensitivity`, and condition-inspection helpers
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api oracle_gap_manifest --locked`
  - `cargo test -p xtask baseline_warns_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
