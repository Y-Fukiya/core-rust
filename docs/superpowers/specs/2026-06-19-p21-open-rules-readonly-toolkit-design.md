# P21 Open Rules Read-Only Toolkit Design

Date: 2026-06-19
Status: approved design

## Purpose

Build the first, read-only phase of a CDISC P21 to CDISC Open Rules conversion
toolkit. This phase ingests P21 rule configuration extracts and a local
`cdisc-org/cdisc-open-rules` checkout, normalizes both into auditable catalogs,
maps P21 rules to existing Open Rules candidates, classifies conversion
feasibility, and reports the result.

This phase does not generate `rule.yml`, positive test data, or negative test
data. Those outputs are deliberately deferred until the catalog, mapping, and
classification outputs are stable enough to review.

## Background

The repository already contains Rust Open Rules oracle harness work. That
harness treats `cdisc-org/cdisc-open-rules` as an oracle-backed conformance
corpus and keeps skipped coverage gaps separate from supported correctness
mismatches. The read-only toolkit must respect that boundary.

The original conversion specification in a local user workspace document
covers the larger future pipeline, including generated CORE-style rules and
generated test data. This design narrows the first implementation to read-only
cataloging, mapping, classification, and reporting.

## Chosen Approach

Add an independent Python companion toolkit under `src/cdisc_rulekit`. The
toolkit communicates with the Rust project only through generated artifacts
under `output/`. It does not call Rust internals, change engine semantics, or
write into `input/cdisc-open-rules/Published`.

Use lightweight Python dependencies:

- Standard library `csv`, `json`, `argparse`, `dataclasses`, `pathlib`, and
  `difflib`.
- `PyYAML` for Open Rules `rule.yml` parsing.
- `pytest` for tests.

Avoid `pandas`, `pydantic`, `rapidfuzz`, and `typer` in the first phase. They
can be added later if real input scale or user ergonomics justify them.

## Scope

In scope:

- P21 CSV ingest and normalization.
- CDISC Open Rules `rule.yml` ingest from a local checkout.
- Default Open Rules scope of `Published` only.
- Optional `--include-unpublished` flag to also scan `Unpublished`.
- Open Rules test data inventory.
- Open Rules `Check` operator and shape inventory.
- P21 to Open Rules mapping with CG ID priority and conservative fuzzy
  candidates.
- Conversion classification using the full future status vocabulary.
- CSV, JSONL, JSON, and Markdown output reports.
- Fixture-based pytest coverage that does not require real P21 extracts or a
  real upstream Open Rules checkout.

Out of scope:

- Writing `rule.yml`.
- Generating positive or negative test data.
- Creating `output/generated_rules`.
- Exporting anything into `input/cdisc-open-rules/Unpublished`.
- Writing into `input/cdisc-open-rules/Published`.
- Running the Rust engine or changing Rust validation semantics.
- Treating fuzzy mapping as proof of native CORE coverage.

## File Layout

Create these files:

```text
pyproject.toml
src/cdisc_rulekit/__init__.py
src/cdisc_rulekit/cli.py
src/cdisc_rulekit/models.py
src/cdisc_rulekit/io_utils.py
src/cdisc_rulekit/load_p21.py
src/cdisc_rulekit/load_open_rules.py
src/cdisc_rulekit/operator_inventory.py
src/cdisc_rulekit/map_rules.py
src/cdisc_rulekit/classify.py
src/cdisc_rulekit/emit.py
src/cdisc_rulekit/reports.py
tests/cdisc_rulekit/fixtures/p21/cdisc_rule_definitions_latest_2204.csv
tests/cdisc_rulekit/fixtures/p21/cdisc_rule_domain_map.csv
tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/rule.yml
tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/_datasets.csv
tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/_variables.csv
tests/cdisc_rulekit/fixtures/open_rules/Published/CORE-000001/positive/01/data/ae.csv
tests/cdisc_rulekit/fixtures/open_rules/Unpublished/CORE-DRAFT-0001/rule.yml
tests/cdisc_rulekit/test_load_p21.py
tests/cdisc_rulekit/test_load_open_rules.py
tests/cdisc_rulekit/test_operator_inventory.py
tests/cdisc_rulekit/test_map_rules.py
tests/cdisc_rulekit/test_classify.py
tests/cdisc_rulekit/test_cli_readonly.py
```

The existing Rust Open Rules fixture directories remain unchanged.

## Data Model

Use dataclasses with explicit serialization helpers. The first phase does not
need runtime validation beyond well-scoped loader checks.

`CanonicalRule` fields:

- `source`: `P21` or `CDISC_OPEN_RULES`.
- `source_rule_id`.
- `p21_rule_id`.
- `core_rule_id`.
- `cdisc_rule_ids`.
- `standard_name`.
- `standard_version`.
- `substandard`.
- `agency`.
- `category`.
- `severity`.
- `p21_rule_type`.
- `core_rule_type`.
- `message`.
- `description`.
- `domains`.
- `classes`.
- `variables`.
- `target`.
- `raw_condition`.
- `parsed_condition`.
- `source_path`.
- `raw_record`.
- `conversion_status`.
- `conversion_reasons`.
- `conversion_confidence`.

`RuleMapping` fields:

- `p21_rule_id`.
- `core_rule_id`.
- `match_type`: `CG_ID`, `EXACT_ID`, `FUZZY`, or `NONE`.
- `confidence`.
- `cdisc_rule_id_overlap`.
- `standard_match`.
- `domain_overlap`.
- `variable_overlap`.
- `message_similarity`.
- `notes`.

`OperatorInventoryItem` fields:

- `core_rule_id`.
- `source_path`.
- `operator`.
- `path`.
- `node_kind`.
- `name_values`.
- `raw_keys`.

## P21 Loading

`load_p21.py` reads P21 CSV files with `csv.DictReader` and normalizes blank,
`NaN`, `nan`, and whitespace-only cells to `None`.

Semicolon-delimited columns become sorted, deduplicated lists where order is not
semantically meaningful:

- `domains`.
- `domain_classes`.
- `publisher_ids_normalized`.
- `cdisc_cg_ids`.

CG IDs are extracted using `\bCG\d{4,}\b` from dedicated CG columns,
publisher-id columns, messages, descriptions, and raw condition fields. The
loader preserves original values in `raw_record` and never invents missing
official IDs.

If a P21 domain map is provided, active mappings are joined using these keys
when present:

```text
config_version
agency
config_name
standard_name
standard_version
rule_id
source_xml_path
```

Inactive mappings are excluded by default. A later CLI flag may include them,
but the read-only MVP can keep that behavior internal until real data requires
the option.

Malformed JSON in `all_attributes_json` or `child_conditions_json` is preserved
as raw strings and recorded as a warning. Loader failures for individual rows
produce `UNSUPPORTED` candidates rather than aborting the whole build when the
row can still be identified.

## Open Rules Loading

`load_open_rules.py` scans a local Open Rules checkout. By default it scans:

```text
Published/**/rule.yml
```

When `--include-unpublished` is passed, it also scans:

```text
Unpublished/**/rule.yml
```

The loader parses YAML safely. Malformed YAML records a warning and continues.
The loader extracts:

- `Core.Id`.
- `Core.Status`.
- `Core.Version`.
- `Core.Description`.
- `Check`.
- `Outcome.Message`.
- `Rule Type`.
- `Scope.Classes.Include`.
- `Scope.Classes.Exclude`.
- `Scope.Domains.Include`.
- `Scope.Domains.Exclude`.
- `Sensitivity`.
- `Authorities[].Organization`.
- `Authorities[].Standards[].Name`.
- `Authorities[].Standards[].Version`.
- `Authorities[].Standards[].Substandard`.
- `Authorities[].Standards[].References[].Rule Identifier.Id`.

CG IDs are extracted from official authority references first and from raw YAML
text as a fallback. Variable names are collected recursively from `Check` nodes
by reading `name` keys. Existing positive and negative test data directories
are inventoried without validating generated data.

## Operator Inventory

The read-only phase emits an operator and shape inventory from Open Rules
`Check` trees. This output is the authority for later generation phases.

The inventory recursively walks dictionaries and lists under `Check`. For each
dictionary node it records:

- Observed operator-like keys.
- The path within the `Check` tree.
- Node shape as a sorted key list.
- Any `name` values visible at the node.
- The source `Core.Id` and file path.

The initial operator detector is conservative. It treats dictionary keys other
than metadata-like keys such as `name`, `value`, `values`, `variable`, `dataset`,
`domain`, `message`, and `description` as operator candidates only when the node
shape suggests expression structure. This inventory is descriptive, not an
execution schema.

## Mapping

`map_rules.py` produces one mapping row per P21 rule.

Tier 1: CG ID match.

- If P21 and Open Rules share at least one `CGxxxx` ID, emit `match_type =
  CG_ID`.
- Base confidence is `0.90`.
- Add small confidence increments for standard, domain, and variable overlap.
- Cap confidence at `1.00`.

Tier 2: fuzzy candidate.

- If no CG ID match exists, compare standard compatibility, domain overlap,
  variable overlap, message similarity, and description similarity.
- Use standard library `difflib.SequenceMatcher` and token overlap.
- Emit `FUZZY` only as a candidate when confidence is at least `0.60`.
- Strong fuzzy candidates may have confidence `0.80` or higher, but they do not
  imply native coverage.

Tier 3: no mapping.

- Emit `NONE` with confidence `0`.

Diagnostic message text is never the primary comparison key. It is one weak
signal after structural fields such as CG ID, standard, domain, and variables.

## Classification

`classify.py` uses the full future status vocabulary:

```text
NATIVE_CORE
AUTO_CONVERTIBLE
TEST_DATA_ONLY
SKELETON_ONLY
MANUAL_REQUIRED
UNSUPPORTED
```

`NATIVE_CORE` is allowed only when:

```text
mapping.match_type == CG_ID
mapping.confidence >= 0.90
core_rule_id is present
```

Fuzzy mappings are never promoted to `NATIVE_CORE`.

`AUTO_CONVERTIBLE` is a future-generation candidate only. The read-only phase
does not generate output folders. A P21 rule can be classified as
`AUTO_CONVERTIBLE` only when all of these are true:

- `p21_rule_type` is one of `Match`, `Regex`, `Condition`, `Required`, or
  `Find`.
- `standard_name` is compatible with `SDTM-IG`, `ADaM-IG`, or `SEND-IG`.
- The row is not a Define.xml, Schematron, or metadata-only rule.
- The row has a clear target or variable.
- The row has a concrete domain or a safe representative domain.
- The condition does not expose cross-domain, external lookup, unresolved macro,
  SUPP--, RELREC, or complex uniqueness dependencies.

`TEST_DATA_ONLY` is reserved for future test-data planning. The initial
read-only classifier recognizes the status value but does not need to emit it.
When a P21 rule has a high-confidence CG ID mapping to an existing Open Rules
rule, `NATIVE_CORE` takes precedence. Fuzzy candidates must not be classified as
`TEST_DATA_ONLY`, because fuzzy mapping is not proof that existing CORE logic is
the intended target for added tests.

`SKELETON_ONLY` is used when metadata is clear but condition semantics are not
safely convertible.

`MANUAL_REQUIRED` is used for Define.xml, Schematron, metadata, Varorder,
Varlength, complex Unique checks, complex Lookup checks, cross-domain checks,
SUPP--, RELREC, ambiguous macros, and empty or unclear condition fields.

`UNSUPPORTED` is used for malformed rows or files that cannot be parsed enough
to classify.

Reason codes are machine-readable and include:

```text
HAS_NATIVE_CORE_MAPPING
FUZZY_CORE_CANDIDATE
NO_CORE_MAPPING
SIMPLE_MATCH_TERMS
SIMPLE_REGEX
SIMPLE_SAME_RECORD_CONDITION
DATASET_PRESENCE_CHECK
DEFINE_XML_RULE
SCHEMATRON_RULE
METADATA_RULE
CROSS_DATASET_DEPENDENCY
EXTERNAL_LOOKUP_DEPENDENCY
UNRESOLVED_VARIABLE_MACRO
NO_CONCRETE_DOMAIN
NO_TARGET_VARIABLE
UNSUPPORTED_RULE_TYPE
MALFORMED_INPUT
OPERATOR_INVENTORY_AVAILABLE
```

## CLI

Expose these commands:

```sh
python -m cdisc_rulekit.cli ingest-p21 \
  --rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --domain-map input/p21/cdisc_rule_domain_map.csv \
  --out output/catalog

python -m cdisc_rulekit.cli ingest-open-rules \
  --repo input/cdisc-open-rules \
  --out output/catalog

python -m cdisc_rulekit.cli ingest-open-rules \
  --repo input/cdisc-open-rules \
  --out output/catalog \
  --include-unpublished

python -m cdisc_rulekit.cli map \
  --p21 output/catalog/p21_rules_normalized.jsonl \
  --core output/catalog/core_rules_normalized.jsonl \
  --out output/mapping

python -m cdisc_rulekit.cli classify \
  --p21 output/catalog/p21_rules_normalized.jsonl \
  --mapping output/mapping/p21_to_core_mapping.jsonl \
  --out output/catalog \
  --reports output/reports

python -m cdisc_rulekit.cli build-readonly \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules \
  --out output \
  --standard SDTM-IG \
  --limit 20
```

Do not expose `generate`, `validate-structure`, or export commands in the
read-only MVP.

## Outputs

Write only under the configured output directory, `output/` by default.

Catalog outputs:

```text
output/catalog/p21_rules_normalized.csv
output/catalog/p21_rules_normalized.jsonl
output/catalog/core_rules_normalized.csv
output/catalog/core_rules_normalized.jsonl
output/catalog/core_testdata_inventory.csv
output/catalog/core_operator_inventory.csv
output/catalog/core_operator_inventory.jsonl
output/catalog/conversion_status.csv
```

Mapping outputs:

```text
output/mapping/p21_to_core_mapping.csv
output/mapping/p21_to_core_mapping.jsonl
```

Report outputs:

```text
output/reports/conversion_status_summary.md
output/reports/readiness_summary.json
```

No `output/generated_rules` directory is created in the read-only phase.

## Testing

Tests use minimal fixtures under `tests/cdisc_rulekit/fixtures`. They do not
require real P21 extracts or a real Open Rules checkout.

Required behavior tests:

- P21 loader normalizes blanks and semicolon lists.
- P21 loader extracts CG IDs without inventing missing IDs.
- P21 loader joins active domain map rows.
- Open Rules loader scans `Published` by default.
- Open Rules loader includes `Unpublished` only when requested.
- Open Rules loader extracts authority CG IDs and fallback raw YAML CG IDs.
- Open Rules loader inventories positive and negative test data.
- Operator inventory records observed `Check` node shapes.
- CG ID mapping gives confidence at least `0.90`.
- Fuzzy mapping emits `FUZZY` without promoting to native coverage.
- No adequate candidate emits `NONE`.
- CG ID mapped rules classify as `NATIVE_CORE`.
- Simple future-generation candidates classify as `AUTO_CONVERTIBLE`.
- Define.xml and Schematron rows classify as `MANUAL_REQUIRED`.
- Malformed rows classify as `UNSUPPORTED`.
- `build-readonly` writes expected catalog, mapping, and report files from
  fixtures.

Run:

```sh
pytest tests/cdisc_rulekit
```

If `PyYAML` or `pytest` is not installed, the developer may install them in a
local virtual environment. The repository should not vendor third-party
packages.

## Migration Path

The Python toolkit remains easy to replace or integrate later because its
contract is artifact-based:

1. Keep Python as a standalone companion tool.
2. Add a Rust `xtask` wrapper that shells out to the Python CLI.
3. Port stable modules to Rust while preserving CSV and JSONL output
   compatibility.
4. Split the toolkit into its own repository or package if it grows beyond this
   workspace.

The first implementation should avoid Python APIs being called directly from
Rust. The artifacts under `output/` are the boundary.

## Risks And Controls

Risk: Fuzzy mapping creates false confidence.

Control: Fuzzy candidates are never classified as `NATIVE_CORE`; they remain
auditable candidates with notes and confidence.

Risk: Read-only classification implies generated rules are already safe.

Control: `AUTO_CONVERTIBLE` means future candidate only in this phase. Reports
must state that no generated `rule.yml` or test data was produced.

Risk: Open Rules unpublished drafts distort native coverage.

Control: Scan `Published` by default. Require `--include-unpublished` to include
drafts.

Risk: Python dependency sprawl complicates a Rust repository.

Control: Use lightweight dependencies in the first phase and keep the CLI
artifact-oriented.

Risk: Fixture-only tests miss real input quirks.

Control: Implement robust warnings and preserve raw records. Run real-data
pilots only after `input/p21` and `input/cdisc-open-rules` are supplied.

## Acceptance Criteria

- `pytest tests/cdisc_rulekit` passes.
- `build-readonly` completes on repository fixtures.
- P21 ingest emits CSV and JSONL normalized catalogs.
- Open Rules ingest emits normalized rule catalog, test data inventory, and
  operator inventory.
- Mapping emits exactly one row per P21 rule.
- Classification emits exactly one row per P21 rule.
- `NATIVE_CORE` appears only for CG ID matches.
- `AUTO_CONVERTIBLE` rows are future-generation candidates only.
- No command writes outside the configured output directory.
- No command writes into `input/cdisc-open-rules/Published`.
- No generated `rule.yml`, positive data, or negative data is produced.

## Self-Review

- The scope is limited to one subsystem: a read-only Python companion toolkit.
- The design keeps generated rule and generated test data work out of the first
  implementation.
- The output contract is explicit and artifact-based.
- Fuzzy mapping cannot become native coverage.
- The design preserves the existing Rust Open Rules oracle harness boundary.
- The file layout, CLI, outputs, and tests are concrete enough for an
  implementation plan.
