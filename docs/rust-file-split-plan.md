# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 11102 | Continue moving Open Rules fixture-style tests into focused modules under `crates/core-api/src/tests/`. |
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
- `core-data/src/dataset_transforms.rs`: first dataset transform split,
  currently stable sort by key columns.
- `core-data/src/usdm_json_schema.rs`: USDM JSON schema issue flattening
  collector and schema message helpers.
- `core-data/src/usdm_references.rs`: USDM id/reference key collection,
  `usdm:tag`/`usdm:ref` parsing, and parameter-map reference validation.
- `core-engine/src/group_operators.rs`: unique-set, relationship, and
  inconsistent-across-dataset operator evaluation.
- `core-engine/src/date_operators.rs`: complete/partial date classification,
  date comparison, and ISO duration validation.
- `core-engine/src/scalar_operators.rs`: scalar comparator resolution,
  placeholder expansion, and scalar equality/list helper logic.
- `core-engine/src/tests.rs`: core-engine validation and operator regression
  tests moved out of `lib.rs`.
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
- `.github/workflows/ci.yml`: PR CI now runs both the repository-local curated
  fixture gate and an expanded pinned curated upstream subset gate covering all
  provenance detail families plus reference distinct, record-count, USDM
  codelist, grouped distinct, and XHTML operation representatives.

## Next Implementation Slice

The next low-risk code slice is:

- move the next cohesive row-scope or match-dataset fixture family from
  `core-api/src/tests.rs` into an existing `tests/open_rules_*.rs` module,
  or split the next pure USDM collector family from `core-data/src/lib.rs`
- prefer code that already has focused tests and does not require behavior
  changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_fails_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
