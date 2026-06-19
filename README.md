# core-rust

Rust implementation of a CDISC CORE-like validation engine.

> Status: technical preview. This project is not an official CDISC product and is
> not a drop-in replacement for the official CDISC Validator.

`core-rust` is an experimental validation engine for working with CDISC-style
rules and SDTM/ADaM-like study data. It is designed for compatibility testing,
supplemental validation workflows, and exploring what a Rust implementation of a
CORE rules engine can look like.

## Where It Helps

- Experimenting with CDISC CORE-like rules in a small, fast Rust codebase.
- Running supplemental validation against SDTM/ADaM-style fixture or study data.
- Comparing Rust behavior with Python/CDISC-compatible expected outputs.
- Producing JSON, CSV, and log reports for traceable validation results.
- Developing validation engine features such as rule normalization, dataset
  operations, JSONATA-like conditions, Define-XML metadata, controlled
  terminology, and external dictionaries.

## What It Is Not

- It is not the official CDISC Validator.
- It is not endorsed by CDISC.
- It is not yet a validated system for regulatory submission workflows.
- It does not guarantee complete behavioral parity with CDISC official tooling
  or the Python `cdisc-rules-engine`.

Use it as a technical preview and supplemental validator, not as the sole
authority for production submission decisions.

## Current Capabilities

- CLI binary: `core-rs`.
- Rule loading from JSON and YAML.
- Record-level and dataset-level rule evaluation.
- Rule include/exclude filters, including skipped results for missing requested
  rules.
- Standard and standard-version filtering.
- Dataset inputs:
  - CSV
  - DatasetPackageJson-style JSON
  - SAS XPT v5 subset
- CDISC metadata inputs:
  - Define-XML parsing for datasets, variables, codelists, value lists, where
    clauses, methods, comments, and documents.
  - Controlled terminology JSON.
  - External dictionaries from JSON and CSV.
- Operations and cross-dataset checks:
  - filter
  - derive
  - aggregate/group statistics
  - sort
  - row number
  - left/inner/semi/anti joins
  - Match_Datasets-style checks
- JSONATA-like rule support for a practical subset of expressions used in the
  compatibility fixtures.
- Reports:
  - `report.json`
  - `report.csv`
  - `validation.log`

## Validation Coverage

The repository includes golden fixtures for:

- integrated DatasetPackageJson + Define-XML + CT flows
- SDTM/ADaM-like multi-domain packages
- regulatory-style SDTM/ADaM fixtures
- Python/CDISC-compatible expected output comparisons
- issue identity traceability with `usubjid` and sequence fields
- CSV and log report structure
- GitHub Actions CI running format, check, and workspace tests

These fixtures make regressions visible, but they are not a substitute for broad
parallel runs against the official CDISC Validator or the Python rules engine on
large real-world study datasets.

## Quick Start

Requires Rust 1.93 or newer.

Build and test:

```sh
cargo check --workspace
cargo test --workspace
```

Run the CLI against the bundled regulatory-style fixture:

```sh
cargo run -p core-cli -- validate \
  --local-rules tests/fixtures/rules/regulatory \
  --dataset-path tests/fixtures/datasets/regulatory/study_package.json \
  --define-xml tests/fixtures/cdisc/regulatory_define.xml \
  --ct tests/fixtures/cdisc/regulatory_ct.json \
  --external-dictionary tests/fixtures/cdisc/regulatory_external_dictionary.csv \
  --log-level info \
  --output target/core-rust-report
```

Expected outputs:

```text
target/core-rust-report/report.json
target/core-rust-report/report.csv
target/core-rust-report/validation.log
```

Show CLI help:

```sh
cargo run -p core-cli -- validate --help
```

## CDISC Rulekit Pilot

The Python `cdisc_rulekit` package provides a Phase 1 read-only pipeline for
normalizing P21 rule exports and CDISC Open Rules into catalogs, candidate
mappings, conversion classifications, and reports. It does not generate or copy
rules in this phase.

The Open Rules input may be either an extracted repository directory or a zip
archive such as `cdisc-open-rules-main.zip`. Zip archives are extracted under
`output/_work/` during read-only commands.

```sh
python -m cdisc_rulekit.cli pilot-preflight \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports \
  --standard SDTM-IG \
  --limit 20

python -m cdisc_rulekit.cli build-readonly \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output \
  --standard SDTM-IG \
  --limit 20
```

Phase 1 reports include:

- `classification_quality.md`
- `macro_inventory.csv`
- `macro_inventory_summary.md`
- `fuzzy_mapping_review.csv`
- `reason_examples.csv`
- `version_agency_summary.csv`
- `raw_rule_id_summary.csv`
- `source_rule_tracking.csv`

Phase 2 minimal generation uses the read-only outputs and conservatively creates
Draft rules only for `AUTO_CONVERTIBLE` rows that are not fuzzy CORE candidates.
Generated rules are written under `generated_rules/`; existing Open Rules
`Published/` content is not modified.

```sh
python -m cdisc_rulekit.cli generate \
  --p21-catalog output/catalog/p21_rules_normalized.jsonl \
  --conversion-status output/catalog/conversion_status.csv \
  --operator-inventory output/catalog/core_operator_inventory.csv \
  --out output

python -m cdisc_rulekit.cli validate-structure \
  --generated-rules output/generated_rules \
  --out output/reports

python -m cdisc_rulekit.cli run-core \
  --generated-rules output/generated_rules \
  --out output \
  --dry-run

python -m cdisc_rulekit.cli run-core \
  --generated-rules output/generated_rules \
  --out output

python -m cdisc_rulekit.cli compare-results \
  --generated-rules output/generated_rules \
  --actual-root output/core_runs \
  --out output/reports

python -m cdisc_rulekit.cli export-rules \
  --generated-rules output/generated_rules \
  --open-rules-repo input/cdisc-open-rules
```

`run-core --dry-run` writes the planned engine commands without executing them.
Non-dry-run execution passes only dataset CSV files from each generated
`positive/01/data` and `negative/01/data` directory to `core-cli`; Open Rules
auxiliary files such as `.env`, `_datasets.csv`, and `_variables.csv` are not
passed as datasets. `compare-results` compares structural fields such as rule
id, dataset/domain, row, variables, USUBJID, and sequence values. Diagnostic
message wording is not a primary comparison key. Actual skipped CORE cases are
reported as `ACTUAL_SKIPPED`, separate from structural mismatches.

`export-rules` copies generated draft rules into
`Unpublished/NEW-RULE/<draft-rule-id>/` by default and writes
`export_manifest.json` / `export_manifest.csv`. Existing target directories are
not overwritten unless `--overwrite` is supplied.

Minimal generated outputs include:

- `generated_rules/<draft-rule-id>/rule.yml`
- `generated_rules/<draft-rule-id>/manifest.json`
- `generated_rules/<draft-rule-id>/expected_results.csv`
- `generated_rules/<draft-rule-id>/positive/01/data/*`
- `generated_rules/<draft-rule-id>/negative/01/data/*`
- `reports/generation_summary.csv`
- `reports/generation_summary.json`
- `reports/structure_validation.md`
- `reports/core_run_plan.json`
- `reports/core_run_plan.md`
- `reports/core_run_execution_summary.csv`
- `reports/core_run_execution_summary.json`
- `reports/core_run_execution_summary.md`
- `reports/comparison_summary.csv`
- `reports/comparison_summary.json`
- `reports/comparison_summary.md`

## Workspace Layout

- `apps/cli`: command-line interface.
- `crates/core-api`: orchestration API for loading inputs, applying filters, and
  running validation.
- `crates/core-rule-model`: rule parsing and normalization.
- `crates/core-data`: dataset loading and operations.
- `crates/core-engine`: rule evaluation.
- `crates/core-cdisc-library`: Define-XML, CT, and dictionary parsing.
- `crates/core-report`: JSON, CSV, and log report writing.
- `tests/fixtures`: golden and compatibility fixtures.

## Compatibility Position

The goal is to move toward Python/CDISC-compatible behavior where practical, but
the project intentionally publishes its current state as a technical preview.
Compatibility should be treated as evidence-based rather than guaranteed:

- If a behavior is covered by a golden fixture, regressions should be caught by
  CI.
- If a behavior is not covered by fixture comparison, treat it as experimental.
- For production or submission-critical workflows, compare results with the
  official validator and established validation processes.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).

## Acknowledgment

This repository uses CDISC, SDTM, ADaM, Define-XML, and related terminology to
describe interoperability goals. Those names belong to their respective owners.
This project is independent and unofficial.
