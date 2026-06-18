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

Requires Rust 1.85 or newer.

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
