# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 17544 | Continue moving Open Rules fixture-style tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 11818 | Continue extracting Open Rules compatibility helpers after the existing `open_rules_compat`, `standard_filter`, `usdm_jsonata`, and `condition_inspect` modules. |
| `crates/core-data/src/lib.rs` | 10361 | Continue extracting USDM JSON flattening and dataset-package helpers after the Open Rules data-dir loader and transform split. |
| `crates/core-engine/src/lib.rs` | 3985 | Continue extracting scalar/operator helper families after the group-operator, date-operator, and scalar-helper splits. |
| `xtask/src/open_rules/score.rs` | 2239 | Continue splitting scoring fixtures after the summary/gate/provenance/policy and identity-normalization splits. |

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
   package helpers after the Open Rules CSV/data-dir loader and dataset
   transform split.
4. `core-engine/src/lib.rs`: continue splitting scalar/operator helper
   families after group/relationship and date/duration evaluators moved out.
5. `xtask/src/open_rules/score.rs`: continue with issue normalization and
   test-fixture helpers after the `ScoreSummary`/`ScoreGate`/provenance,
   deferred-policy, and identity-normalization split.

## Completed Slices

- `core-api/src/open_rules_compat/classifier.rs`: oracle-gap classifier
  predicates and post-operator skip classification.
- `core-api/src/condition_inspect.rs`: pure condition tree inspection helpers
  used by Open Rules compatibility classification.
- `core-api/src/tests/open_rules_data_loader.rs`: first Open Rules data-loader
  and row-scope regression tests.
- `core-api/src/tests/open_rules_usdm.rs`: USDM/Open Rules JSONata and USDM
  join regression tests.
- `core-api/src/tests/open_rules_dates.rs`: Open Rules date, partial-date,
  duration, and date ordering regression tests.
- `core-api/src/tests/basic_validation.rs`: basic rule selection,
  preflight, and report-writing API tests.
- `core-data/src/open_rules_data_dir.rs`: Open Rules `_datasets.csv`,
  `_variables.csv`, embedded metadata, and CSV data-dir loading.
- `core-data/src/dataset_transforms.rs`: first dataset transform split,
  currently stable sort by key columns.
- `core-data/src/usdm_json_schema.rs`: USDM JSON schema issue flattening
  collector and schema message helpers.
- `core-engine/src/group_operators.rs`: unique-set, relationship, and
  inconsistent-across-dataset operator evaluation.
- `core-engine/src/date_operators.rs`: complete/partial date classification,
  date comparison, and ISO duration validation.
- `core-engine/src/scalar_operators.rs`: scalar comparator resolution,
  placeholder expansion, and scalar equality/list helper logic.
- `xtask/src/open_rules/score/policy.rs`: deferred oracle-gap score policy
  and reason mapping.
- `xtask/src/open_rules/score/normalization.rs`: deferred oracle-gap issue
  identity normalizations such as row-locator relaxation and output-context
  variable alignment.
- `xtask/src/open_rules/score/summary.rs`: scoreboard summary, deferred
  oracle-gap breakdown, group summaries, and score gate policy.
- `xtask/src/open_rules/score/provenance.rs`: candidate provenance parsing
  and detailed execution-provenance classification.
- `xtask/src/open_rules/score/identity.rs`: row/sequence locator
  normalization and duplicate sequence detection for Open Rules score
  comparison.

## Next Implementation Slice

The next low-risk code slice is:

- move another cohesive Open Rules test family from `core-api/src/tests.rs`,
  or split the next pure USDM collector family from `core-data/src/lib.rs`
- prefer code that already has focused tests and does not require behavior
  changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_fails_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
