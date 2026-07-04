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
- Comparing Rust behavior with golden expected outputs.
- Producing JSON, CSV, and log reports for traceable validation results.
- Developing validation engine features such as rule normalization, dataset
  operations, targeted hand-ported USDM/Open Rules checks, Define-XML metadata, controlled
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
- JSONata normalization for a small expression subset plus targeted
  hand-ported USDM/Open Rules checks. These hand ports are tracked separately
  in Open Rules provenance and should not be read as a general JSONata
  evaluator.
- Reports:
  - `report.json`
  - `report.csv`
  - `validation.log`

## Validation Coverage

The repository includes golden fixtures and Open Rules oracle harnesses for:

- integrated DatasetPackageJson + Define-XML + CT flows
- SDTM/ADaM-like multi-domain packages
- regulatory-style SDTM/ADaM fixtures
- golden expected output comparisons
- issue identity traceability with `usubjid` and sequence fields
- CSV and log report structure
- GitHub Actions CI running format, check, and workspace tests
- CDISC Open Rules fixture and curated-upstream subset comparisons

Open Rules coverage metrics are split by evidence type. `supported_accuracy`
only measures cases still in the supported denominator after reviewed
`deferred_oracle_gap_*`, missing-oracle, and unsupported cases have been
excluded. It is a regression-gate invariant, not a claim that the full upstream
corpus is completely implemented. Read it together with `coverage`,
`native_engine_coverage`, `rule_id_hand_port_coverage`,
`deferred_oracle_gap_skipped`, and `no_official_oracle`.

Validation `report.json` includes per-result `execution_provenance` when the
API can classify the rule path (`native_engine` or `rule_id_hand_port`). This is
intended for auditability; CSV output keeps the stable issue-row schema.

For audit runs, `xtask open-rules score --strict-scoring` disables oracle-gap
reclassification and oracle-informed identity/output-context normalizations.
That strict score is intentionally harsher and is the right lens for estimating
how much the compatibility scorer, manifests, and normalization policy affect
the headline gate metrics. The scheduled upstream observe workflow uploads both
the default compatibility scoreboard and a non-gating strict-scoring scoreboard
so the two reads can be compared.

These fixtures make regressions visible, but they are not a substitute for broad
parallel runs against the official CDISC Validator or the Python rules engine on
large real-world study datasets.

Release artifacts should be accompanied by a provenance manifest generated with
`cargo run -p xtask -- release-manifest --out <path>`. See
[`docs/release-reproducibility.md`](docs/release-reproducibility.md) for the
release checklist and reproducibility notes.

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

# Expanded draft generation. This keeps fuzzy CORE candidates as draft P21PORT
# rules and records FUZZY_CORE_CANDIDATE_REQUIRES_REVIEW in manifest.json.
python -m cdisc_rulekit.cli generate \
  --p21-catalog output/catalog/p21_rules_normalized.jsonl \
  --conversion-status output/catalog/conversion_status.csv \
  --operator-inventory output/catalog/core_operator_inventory.csv \
  --out output-expanded \
  --include-fuzzy-candidates

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

# Official Python CORE source checkout. Use absolute paths when --engine-cwd is
# set because CORE reads resources relative to its repository root.
CORE_RUST_ROOT="$PWD"
CORE_ENGINE="$CORE_RUST_ROOT/input/cdisc-rules-engine"
python -m cdisc_rulekit.cli run-core \
  --generated-rules "$CORE_RUST_ROOT/output/generated_rules" \
  --out "$CORE_RUST_ROOT/output/core_cli_run" \
  --engine-command "$CORE_ENGINE/.venv/bin/python $CORE_ENGINE/core.py validate -s SDTMIG -v 3.2 --output-format json -p disabled" \
  --output-mode file-base \
  --data-mode data-dir \
  --engine-cwd "$CORE_ENGINE" \
  --workers 6

python -m cdisc_rulekit.cli compare-results \
  --generated-rules output/generated_rules \
  --actual-root output/core_runs \
  --out output/reports

python -m cdisc_rulekit.cli export-rules \
  --generated-rules output/generated_rules \
  --open-rules-repo input/cdisc-open-rules

# Export only comparison-passed rules to a dedicated target subtree (recommended for PR-ready candidates)
python -m cdisc_rulekit.cli export-rules \
  --generated-rules output/generated_rules \
  --open-rules-repo input/cdisc-open-rules \
  --comparison-summary output/reports/comparison_summary.csv \
  --only-passed \
  --target-subdir Unpublished/NEW-RULE/FINAL-PASS
```

`run-core --dry-run` writes the planned engine commands without executing them.
By default, non-dry-run execution passes only dataset CSV files from each
generated `positive/01/data` and `negative/01/data` directory to `core-cli`;
Open Rules auxiliary files such as `.env`, `_datasets.csv`, and
`_variables.csv` are not passed as datasets. For the official Python CORE CLI,
use `--data-mode data-dir` so the full Open Rules test data directory is passed
with `_datasets.csv` / `_variables.csv`, and `--output-mode file-base` so
`report.json` / `report.csv` lands under the case output directory. `--workers`
runs case-level CORE invocations in parallel; keep this moderate because the
official CORE CLI also initializes its own validation machinery per process.
`compare-results` compares structural fields such as rule id, dataset/domain,
row, variables, USUBJID, and sequence values. It supports both the Rust harness
`results/errors` JSON shape and official Python CORE `Issue_Details` JSON
shape. Diagnostic message wording is not a primary comparison key. Actual
skipped CORE cases are reported as `ACTUAL_SKIPPED`, separate from structural
mismatches.

`export-rules` copies generated draft rules into
`Unpublished/NEW-RULE/<draft-rule-id>/` by default and writes
`export_manifest.json` / `export_manifest.csv`. Existing target directories are
not overwritten unless `--overwrite` is supplied.

Historical SDTM-IG pilot result:

- Successful run directory: `output/sdtmig_phase2_rerun5`
- Generated high-confidence draft rules: 17
- CORE execution comparison: 34 passed, 0 failed
- Remaining skipped rows are generation-scope coverage gaps, not supported CORE
  mismatches. Keep them separate from wrong results when expanding generation.

Older SDTM-IG full rerun reports under `output/` were produced before the
Open Rules oracle scorer was tightened to exclude missing-official-oracle cases
from supported accuracy and before `core-api` stopped oracle-based result
synthesis. Treat those reports as historical debugging artifacts only. For
current conformance claims, rerun the Open Rules harness and cite the generated
`scoreboard.json` / `summary.md` from that run.

Latest expanded SDTM-IG draft export:

- Expanded run directory: `output/sdtmig_phase2_condition_target_v2`
- Generated draft rules: 395
- Structure validation: 395 rules checked, ok
- Export target:
  `output/_work/open_rules_zip/cdisc-open-rules-main/Unpublished/NEW-RULE/P21PORT-SDTMIG-CONDITION-TARGET`
- Export result: 395 exported, 0 skipped
- `Published/` was not modified. Fuzzy-derived rules remain draft/review items
  through the `FUZZY_CORE_CANDIDATE_REQUIRES_REVIEW` manifest warning.

Official Python CORE smoke:

- CORE source checkout: `input/cdisc-rules-engine` at commit `b8202fe`
- Installed into `input/cdisc-rules-engine/.venv` with Python 3.12
- `core.py test-validate json`: passed when run from the CORE repository root
- `run-core` against one generated rule with official CORE options: 2 passed,
  0 failed
- `compare-results` against that official CORE output: 2 passed, 0 failed

Historical official Python CORE full run:

The Python CORE run reports under `output/` are also historical. They are useful
for reproducing the P21 conversion workflow, but they are not a substitute for
the Rust Open Rules oracle scoreboard after the scorer hardening changes.

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

The goal is to make behavior reproducible against reviewed golden fixtures where
practical, but the project intentionally publishes its current state as a
technical preview. Compatibility should be treated as evidence-based rather than
guaranteed:

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
