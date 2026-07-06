# core-rust

[English](README.md) | [日本語](README.ja.md)

`core-rust` is a technical-preview validation engine for CDISC-style rules and
study data. It provides a Rust CLI, report writers, and compatibility harnesses
for supplemental validation, regression testing, and rule-conversion research.

> Status: technical preview. This project is independent and unofficial. It is
> not the official CDISC Validator, not endorsed by CDISC, and not a sole
> authority for regulatory submission decisions.

## Who This Is For

Use `core-rust` when you need to:

- run supplemental checks over SDTM/ADaM-like datasets
- test CDISC CORE-like rules in JSON or YAML
- compare candidate behavior with reviewed golden outputs
- inspect machine-readable JSON/CSV/log validation reports
- research P21PORT and CDISC Open Rules conversion workflows
- audit Open Rules compatibility with provenance-aware scoreboards

Do not use it as a drop-in replacement for the official CDISC Validator.
Submission-critical workflows should still be checked with the official
validator and your governed validation process.

## Install And Build

Requirements:

- Rust 1.93 or newer
- Python 3.11 or newer for the optional `cdisc_rulekit` utilities.
  CI currently tests Python 3.13.

```sh
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --release -p core-cli
```

The CLI binary is named `core-rs`.

```sh
cargo run -p core-cli -- validate --help
```

## Run A Validation

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

Outputs:

```text
target/core-rust-report/report.json
target/core-rust-report/report.csv
target/core-rust-report/validation.log
```

`report.json` includes `execution_provenance` when the engine can classify the
rule path as `native_engine` or `rule_id_hand_port`. CSV keeps a stable
issue-row schema for downstream tools.

## Supported Inputs

Rules:

- JSON
- YAML

Data:

- CSV
- DatasetPackageJson-style JSON
- SAS XPT v5 subset

DatasetPackageJson numbers larger than JavaScript's safe integer range may be
loaded as strings to avoid silent precision loss.

XPT support is a bounded v5 parser subset. Use official tooling for
submission-grade XPORT transport validation.

Metadata:

- Define-XML datasets, variables, codelists, value lists, where clauses,
  methods, comments, and documents
- controlled terminology JSON
- external dictionaries from JSON or CSV

## What The Engine Can Evaluate

The current engine supports record-level and dataset-level checks, filters,
derivations, aggregate/group statistics, sorting, row numbers, joins,
Match_Datasets-style checks, codelist checks, Define-XML metadata checks, and a
small normalized expression subset.

USDM/Open Rules support includes targeted hand-ported checks. These are tracked
separately in Open Rules provenance and should not be read as general JSONata
support.

## Open Rules Compatibility

The repository includes a CDISC Open Rules oracle harness. Scoreboards separate:

- supported matches and mismatches
- deferred oracle/fixture gaps
- no-official-oracle cases
- skipped unsupported cases
- native engine vs rule-id hand-port coverage
- strict identity scoring vs compatibility normalization

`supported_accuracy = 100%` means no mismatch inside the reviewed supported
denominator. It does not mean the full upstream corpus is implemented or that
the tool is regulatory-ready.

For audit runs:

```sh
cargo run -p xtask -- open-rules score --strict-scoring --help
cargo run -p xtask -- open-rules score-delta --help
```

The scheduled upstream workflow uploads default scoreboards, strict scoreboards,
and default-vs-strict delta artifacts.

## P21PORT Rulekit

The optional Python `cdisc_rulekit` package helps inspect authorized,
user-supplied P21-style rule catalog CSVs, classify conversion candidates,
generate draft P21PORT rules, run candidate rules, and compare structural
outputs.

P21PORT does not fetch, scrape, or export proprietary Pinnacle 21 rule
definitions. In particular, do not assume Pinnacle 21 Community can provide the
rule definition CSVs used by `--p21-rules`; bring only rule catalogs you are
licensed and permitted to use.

If you use public Pinnacle 21 Community configuration sources such as
`p21-community/configs`, review the applicable Pinnacle 21 license first.
Generated catalogs or adapted rules should remain local/user-supplied artifacts
unless your license permits sharing them.

For local XML configuration files that you are permitted to process, use the
XML-to-catalog converter before `build-readonly`:

```sh
PYTHONPATH=src python3 -m cdisc_rulekit.cli convert-p21-config \
  --input /path/to/local/p21-config.xml \
  --source-label sdtm33 \
  --out target/p21-config-catalog

PYTHONPATH=src python3 -m cdisc_rulekit.cli build-readonly \
  --p21-rules target/p21-config-catalog/p21_rules_normalized.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports
```

The converter writes `p21_rules_normalized.csv`,
`p21_rules_normalized.jsonl`, and an `extraction_report.md`. It is a
best-effort extractor for local review, not a schema-complete Pinnacle 21
configuration converter. It does not download configuration files, and the
generated catalog should not be committed or shared unless your license permits
it. Review the generated CSV/JSONL before using it as a P21PORT catalog. Use
`--source-label` when you need a stable non-path source identifier for long-term
catalog comparisons; release or longitudinal comparison workflows should prefer
explicit labels over the default input-order labels.

XML parsing uses `defusedxml` in installed environments. Source-tree smoke
tests may fall back to the Python standard-library parser before optional
dependencies are installed, but the converter still rejects DTD/entity
declarations before parsing and reports malformed or unreadable XML as stable
`error: ...` CLI messages. The terminal error may include the full local input
path to disambiguate same-named files; sanitize stderr before sharing logs
externally.

```sh
python -m pip install -e ".[test]"
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
```

Typical read-only pilot:

```sh
python -m cdisc_rulekit.cli pilot-preflight \
  --p21-rules input/p21/cdisc_rule_definitions_latest_2204.csv \
  --p21-domain-map input/p21/cdisc_rule_domain_map.csv \
  --open-rules-repo input/cdisc-open-rules-main.zip \
  --out output/reports \
  --standard SDTM-IG \
  --limit 20
```

P21PORT outputs are draft/review artifacts, not a Pinnacle 21 replacement.
Existing Open Rules `Published/` content is not modified unless you explicitly
export into a target tree.

## CLI Exit Policy

`core-rs validate` writes reports and exits `0` when validation execution
completes, even if the report contains failed or skipped rule results. This
default keeps ad hoc review runs from failing before the report can be inspected.

For CI or release gates, use one of the explicit fail policies:

```sh
core-rs validate ... --fail-on failed
core-rs validate ... --fail-on failed,skipped
core-rs validate ... --strict
```

`--strict` is equivalent to failing on both failed and skipped results. A
non-zero exit from these modes means the report was generated but the requested
validation result policy was not satisfied.

## Release And Audit Artifacts

Release artifacts should be accompanied by a provenance manifest:

The commands below are a local smoke example. For reviewed release bundles, use
the stricter policy flags described after the example. Run these commands from
the repository root so `--source-root .` resolves the reviewed `Cargo.lock`.

```sh
cargo build --release -p core-cli
mkdir -p target/release-provenance/bin
cp target/release/core-rs target/release-provenance/bin/core-rs
cargo run -p xtask -- release-manifest \
  --out target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --source-root . \
  --artifact target/release-provenance/bin/core-rs
cargo run -p xtask -- release-verify \
  --manifest target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --source-root . \
  --require-artifact \
  --require-cargo-lock
```

For reviewed release bundles, use the stricter command in
[Release reproducibility](docs/release-reproducibility.md) with
`--artifact`, `--artifact-root`, `--source-root`, `--require-artifact`,
`--require-cargo-lock`, and other verification policy flags. Local smoke checks
cover artifact existence and hashes; reviewed release verification should also
require the expected target triple, clean git provenance, CI run metadata, and
`SOURCE_DATE_EPOCH`.

The CI release provenance gate builds the host `core-rs` binary, records its
SHA-256 in `release-manifest.json`, verifies the manifest, and uploads the
manifest as a GitHub Actions artifact.

See:

- [Release reproducibility](docs/release-reproducibility.md)
- [Open Rules oracle harness](docs/open-rules-oracle-harness.md)
- [Open Rules upstream regression gate](docs/open-rules-upstream-regression-gate.md)
- [XPT fuzzing](docs/xpt-fuzzing.md)
- [Rust file split plan](docs/rust-file-split-plan.md)

## Workspace Layout

- `apps/cli`: command-line interface
- `crates/core-api`: validation orchestration API
- `crates/core-rule-model`: rule parsing and normalization
- `crates/core-data`: dataset loading and dataset operations
- `crates/core-engine`: rule evaluation
- `crates/core-cdisc-library`: Define-XML, CT, and dictionary parsing
- `crates/core-report`: JSON, CSV, and log report writing
- `src/cdisc_rulekit`: Python P21/Open Rules conversion utilities
- `tests/fixtures`: golden and compatibility fixtures

## License

MIT License. See [LICENSE](LICENSE).

## Acknowledgment

This repository uses terms such as CDISC, SDTM, ADaM, and Define-XML to describe
interoperability. Those names belong to their respective owners. This project is
an independent, unofficial implementation.
