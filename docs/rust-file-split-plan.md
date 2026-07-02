# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 10463 | Continue moving Open Rules fixture-style tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 9363 | Continue extracting Open Rules compatibility helpers after the CDISC context, static codelist, and operation-field helper splits. |
| `crates/core-data/src/lib.rs` | 8451 | Continue extracting USDM JSON flattening and dataset-package helpers after the Open Rules data-dir loader, transform, reference, and test splits. |
| `crates/core-engine/src/lib.rs` | 1779 | Continue extracting remaining operator helpers after the group-operator, date-operator, scalar-helper, and test splits. |
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
   `open_rules_compat/` and sibling modules. The oracle-gap classifier,
   condition-inspection, CDISC context, static codelist, and operation-field
   helper slices have already moved out of `lib.rs`.
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
- `core-api/src/tests/open_rules_dates.rs`: Open Rules date, partial-date,
  duration, and date ordering regression tests.
- `core-api/src/tests/open_rules_metadata.rs`: domain presence, dataset
  metadata, variable metadata, library metadata, and Define metadata tests.
- `core-api/src/tests/open_rules_operations.rs`: reference distinct, grouped
  aggregate, domain label, XHTML, DY, and match-dataset operation tests.
- `core-api/src/tests/open_rules_codelists.rs`: static CDISC codelist,
  package-version scoping, Define-XML/CT enrichment, and entity codelist
  operation tests.
- `core-api/src/tests/open_rules_entities.rs`: entity-scope execution,
  entity column-ref oracle-gap, and entity literal fallback tests.
- `core-api/src/tests/open_rules_jsonata.rs`: JSONata normalization,
  JSONata string expression, and unsupported JSONata preflight tests.
- `core-api/src/tests/open_rules_match_datasets.rs`: basic Open Rules
  match-dataset execution, suffix/left-right key joins, single match-dataset
  joins, and multi-match entity joins.
- `core-api/src/tests/basic_validation.rs`: basic rule selection,
  preflight, and report-writing API tests.
- `core-api/src/cdisc_context.rs`: Define-XML, controlled terminology, and
  external dictionary context loading plus codelist comparator enrichment.
- `core-api/src/static_codelists.rs`: static CDISC codelist registry,
  package-version scoping helpers, and term lookup helpers.
- `core-api/src/operation_fields.rs`: Open Rules operation name, key
  normalization, string/list/map field extraction, and expression literal
  parsing helpers.
- `core-data/src/tests.rs`: core-data loader, XPT, join, transform, and Open
  Rules data-dir regression tests moved out of `lib.rs`.
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

- move the next cohesive row-scope or remaining match-dataset fixture family
  from `core-api/src/tests.rs` into an existing `tests/open_rules_*.rs`
  module,
  or split the next pure USDM collector family from `core-data/src/lib.rs`
- prefer code that already has focused tests and does not require behavior
  changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_fails_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
