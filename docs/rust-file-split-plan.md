# Rust File Split Plan

The Open Rules work has improved scoring fidelity, but several Rust files are
still too large to review safely:

| File | Current lines | First split target |
|---|---:|---|
| `crates/core-api/src/tests.rs` | 8728 | Continue moving Open Rules fixture-style tests into focused modules under `crates/core-api/src/tests/`. |
| `crates/core-api/src/lib.rs` | 6780 | Continue extracting Open Rules compatibility helpers after the CDISC context, static codelist, operation-field, metadata-support, operation-execution, metadata-execution, scope-filter, operation-reference, execution-provenance, domain-presence, split-domain unique-set, and result override helper splits. |
| `crates/core-data/src/lib.rs` | 4965 | Continue extracting USDM collector and row-builder helpers after the product/administration, design, content/timeline/scheduled-instance, geography/governance, abbreviation, object, JSON table, population-column, value helper, data-dir loader, transform, reference, and test splits. |
| `crates/core-api/src/tests/open_rules_usdm.rs` | 2100 | Continue splitting USDM fixture families into focused sibling modules. |
| `crates/core-engine/src/lib.rs` | 1778 | Continue extracting remaining operator helpers after the group-operator, date-operator, scalar-helper, and test splits. |
| `xtask/src/open_rules/score.rs` | 572 | Keep the score entrypoint thin; move any new score behavior into focused `score/` modules. |

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
   condition-inspection, CDISC context, static codelist, operation-field,
   metadata-support, operation-execution, metadata-execution, scope-filter,
   operation-reference, execution-provenance, domain-presence, split-domain
   unique-set, and result override helper slices have already moved out of
   `lib.rs`.
2. `core-api/src/tests.rs`: continue moving Open Rules fixture-style tests into
   `tests/open_rules_*.rs` modules. Loader/row-scope and USDM slices have moved
   out already.
3. `core-data/src/lib.rs`: continue with USDM JSON flattening or dataset
   package helpers after the Open Rules CSV/data-dir loader split.
4. `core-engine/src/lib.rs`: continue splitting scalar/date/operator helper
   families after group/relationship operator evaluators moved out.
5. `xtask/src/open_rules/score.rs`: keep as a small orchestration module after
   the summary/gate/provenance/policy/normalization/test splits.

## Completed Slices

- `core-api/src/open_rules_compat/classifier.rs`: oracle-gap classifier
  predicates and post-operator skip classification.
- `core-api/src/condition_inspect.rs`: pure condition tree inspection helpers
  used by Open Rules compatibility classification.
- `core-api/src/tests/open_rules_data_loader.rs`: first Open Rules data-loader
  and row-scope regression tests.
- `core-api/src/tests/open_rules_usdm.rs`: USDM/Open Rules JSONata and USDM
  join regression tests.
- `core-api/src/tests/open_rules_usdm_abbreviations.rs`: USDM abbreviation
  duplicate text and expanded-text regression tests.
- `core-api/src/tests/open_rules_usdm_activity.rs`: USDM activity child id,
  children/detail conflict, child ordering, and biomedical concept/category
  overlap regression tests.
- `core-api/src/tests/open_rules_usdm_blinding.rs`: USDM blinding-schema,
  masked-role, double-blind, and open-label masking regression tests.
- `core-api/src/tests/open_rules_usdm_identifiers.rs`: USDM id spacing and
  identifier scope uniqueness regression tests.
- `core-api/src/tests/open_rules_usdm_narrative.rs`: USDM narrative content
  JSONata regression tests.
- `core-api/src/tests/open_rules_usdm_population.rs`: USDM planned
  enrollment and completion population/cohort consistency regression tests.
- `core-api/src/tests/open_rules_usdm_references.rs`: USDM reference,
  duplicate-object, and broad cross-entity reference regression tests.
- `core-api/src/tests/open_rules_usdm_schema.rs`: USDM JSON schema check pass
  and fail regression tests.
- `core-api/src/tests/open_rules_usdm_study_design.rs`: USDM study-design
  document-version, duplicate code-list, and single/multi-centre regression
  tests.
- `core-api/src/tests/open_rules_usdm_timeline.rs`: USDM main timeline,
  planned duration, and timeline ordering regression tests.
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
  joins, multi-match entity joins, and missing-left match-dataset joins.
- `core-api/src/tests/basic_validation.rs`: basic rule selection,
  preflight, and report-writing API tests.
- `core-api/src/cdisc_context.rs`: Define-XML, controlled terminology, and
  external dictionary context loading plus codelist comparator enrichment.
- `core-api/src/static_codelists.rs`: static CDISC codelist registry,
  package-version scoping helpers, and term lookup helpers.
- `core-api/src/operation_fields.rs`: Open Rules operation name, key
  normalization, string/list/map field extraction, and expression literal
  parsing helpers.
- `core-api/src/metadata_support.rs`: metadata rule support predicates,
  operation support predicates, group alias checks, and operation dataset-name
  helpers.
- `core-api/src/operation_execution.rs`: Open Rules operation column
  resolution, grouped count/distinct derivation, group-key normalization, and
  inline operation filtering helpers.
- `core-api/src/metadata_execution.rs`: metadata execution helper tables,
  model variable lists, metadata operation value insertion, and domain-list
  helpers used by dataset/variable/value metadata execution.
- `core-api/src/scope_filter.rs`: domain/entity/class scope filtering,
  scope wildcard matching, and SDTM class lookup helpers.
- `core-api/src/operation_references.rs`: Open Rules operation reference-value
  expansion, optional reference expansion, and dataset filtered-variable
  derivation helpers.
- `core-api/src/execution_provenance.rs`: per-result execution provenance
  annotation used by API/report JSON output.
- `core-api/src/domain_presence.rs`: domain-presence execution dataset
  construction and variable-exists operation projection.
- `core-api/src/split_domain_unique_set.rs`: CORE-000750 split-domain
  unique-set issue construction and scope filtering.
- `core-api/src/result_overrides.rs`: skipped-result construction,
  oracle-gap result overrides, missing dataset/scope result construction, and
  preflight unsupported operator/operation helpers.
- `core-data/src/tests.rs`: core-data loader, XPT, join, transform, and Open
  Rules data-dir regression tests moved out of `lib.rs`.
- `core-data/src/open_rules_data_dir.rs`: Open Rules `_datasets.csv`,
  `_variables.csv`, embedded metadata, and CSV data-dir loading.
- `core-data/src/json_table.rs`: JSON record-to-DataFrame conversion plus
  USDM/Open Rules JSON row dataset wrapping.
- `core-data/src/usdm_values.rs`: shared USDM JSON scalar, list, code, and
  quantity formatting helpers.
- `core-data/src/usdm_population_columns.rs`: USDM population/cohort quantity
  and planned-sex derived column helpers.
- `core-data/src/usdm_abbreviations.rs`: USDM abbreviation row collection,
  row building, and duplicate-text flags.
- `core-data/src/usdm_objects.rs`: recursive USDM object row collection and
  duplicate id/name flagging.
- `core-data/src/usdm_geography.rs`: USDM geographic-scope collection,
  governance-date collection, global duplicate type detection, and associated
  row builders.
- `core-data/src/usdm_content.rs`: USDM narrative content,
  document-content-reference, schedule timeline, and scheduled-instance
  collection plus associated row builders.
- `core-data/src/usdm_timeline.rs`: USDM timeline-ordering and object-label
  formatting helpers used by study-design row construction.
- `core-data/src/usdm_design.rs`: USDM study-design, interventional-design,
  duplicate design-list, intervention reference, blinding-role, and primary
  endpoint row construction.
- `core-data/src/usdm_product.rs`: USDM administrable product, administration,
  ingredient strength, amendment reason, and product organization role
  collection plus row construction.
- `core-engine/src/group_operators.rs`: unique-set, relationship, and
  inconsistent-across-dataset operator evaluation.
- `xtask/src/open_rules/score/summary.rs`: scoreboard summary, deferred
  oracle-gap breakdown, group summaries, and score gate policy.
- `xtask/src/open_rules/score/provenance.rs`: candidate provenance parsing
  and detailed execution-provenance classification.
- `xtask/src/open_rules/score/normalization.rs`: score-only identity
  normalization helpers for deferred oracle-gap comparison.
- `xtask/src/open_rules/score/tests.rs`: score fixture construction and
  scoreboard/bucket policy regression tests moved out of `score.rs`.

## Next Implementation Slice

The next low-risk code slice is:

- split the next pure USDM collector or row-builder family from
  `core-data/src/lib.rs`, or extract another pure metadata/operation helper
  family from `core-api/src/lib.rs`
- prefer code that already has focused tests and does not require behavior
  changes
- keep public behavior unchanged
- verify with:
  - `cargo test -p core-api open_rules --locked`
  - `cargo test -p xtask baseline_fails_when_deferred_oracle_gap_skipped_increases --locked`
  - `cargo check --workspace --locked`
