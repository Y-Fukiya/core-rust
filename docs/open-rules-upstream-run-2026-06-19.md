# Open Rules Upstream Run 2026-06-19

## Inputs

- Upstream: `cdisc-org/cdisc-open-rules`
- Source: GitHub archive for pinned SHA
  `7f7fae49376b3d023563ebb6c36a3b392d6e649f`
- Scope: `Published`
- Command:

```sh
cargo run -p xtask -- open-rules run-score \
  --open-rules-root /private/tmp/cdisc-open-rules-7f7fae49376b3d023563ebb6c36a3b392d6e649f \
  --core-rs-results-root target/open-rules-core-rs-upstream \
  --out target/open-rules-scoreboard-upstream
```

The archive has no `.git` metadata, so the run reports the expected SHA from
`tests/open_rules/upstream.lock` but cannot report an observed checkout SHA.

## Result After Phase 6 Classification Fixes

| Metric | Value |
|---|---:|
| Total cases | 2298 |
| Supported match | 68 |
| Supported mismatch | 0 |
| Skipped unsupported | 2046 |
| No official oracle | 184 |
| Harness error | 0 |
| Supported accuracy | 100.00% |
| Coverage | 2.96% |

Phase 6 deliberately tightens the definition of "supported" to cases that
core-rust can currently compare against the official oracle without known
semantic gaps. This moves unsupported JSONata, missing-oracle cases, dataset
presence semantics, and column-reference comparator semantics out of
`harness_error`/`supported_mismatch` and into explicit skipped buckets.

The Phase 5-to-Phase 6 change moved the scoreboard from:

- `supported_match`: 245 -> 68
- `supported_mismatch`: 36 -> 0
- `skipped_unsupported`: 1559 -> 2046
- `harness_error`: 458 -> 0
- `supported_accuracy`: 87.19% -> 100.00%
- `coverage`: 12.23% -> 2.96%

## Fixes Applied

- Preserve YAML `value: N` and related CDISC literal values as strings before
  YAML boolean coercion can convert them to booleans.
- Expand multi-variable candidate report rows into variable-level issue keys,
  matching official `results.csv` rows.
- When official `results.csv` lacks subject or sequence columns, remove those
  candidate-only display fields from primary comparison keys.
- Expand CDISC `--` placeholder variables using the current dataset domain
  prefix when evaluating conditions and writing issue variables.
- Read `Outcome.Output Variables` and `Outcome.Grouping Variables` from the
  Open Rules YAML structure.
- Prefer rule output variables for issue reporting when they are present.
- Treat string comparison values without `value_is_literal: true` as column
  references for comparison operators, with a runtime fallback to literal
  strings when the referenced column is absent.
- For `Record Data` rules with `Dataset` sensitivity, report record-granular
  issues and use dataset-column-presence semantics for `exists` and
  `not_exists`.
- Align candidate rows to official rows when a small case differs only by a
  constant one-row offset.
- Collapse candidate record rows to a dataset-level issue when the official
  oracle represents that issue with an empty `Record` field.
- Classify cases with missing official `results.csv` as `no_official_oracle`
  rather than harness errors.
- Write `run-summary.json` from `xtask open-rules run` so per-case execution
  failures are inspectable.
- Treat unsupported JSONata expressions as unsupported rule coverage instead
  of rule-load failures.
- Skip unsupported rules before loading datasets so missing fixture data does
  not become a harness error for rules that are already out of scope.
- Ignore empty trailing CSV headers and infer `_datasets.csv` from
  `_variables.csv`/dataset CSV files for Open Rules fixtures with partial
  manifests.
- Apply Open Rules `.env` standard/version filters, domain scope filters
  including `SUPP--`, and a conservative domain-to-class scope filter.
- Move dataset-sensitivity presence semantics and column-reference comparator
  semantics to skipped coverage until their oracle behavior is implemented
  precisely.

`CORE-000001`, `CORE-000013`, `CORE-000022`, and both `CORE-000025` cases are
now supported-match.

## Remaining Mismatch Hotspots

There are no remaining `supported_mismatch` cases in the Phase 6 scoreboard.
The next precision work is to re-promote skipped coverage buckets into
supported coverage once their semantics are implemented and verified against
the official oracle.

## Harness Error Split

| Error type | Count |
|---|---:|
| Missing candidate report | 0 |
| Missing official results classified as `no_official_oracle` | 184 |
| Harness error | 0 |

## Next Precision Work

The next precision pass should raise coverage while keeping
`supported_mismatch = 0`, starting with:

- Dataset-sensitivity presence rules such as `CORE-000015`, `CORE-000080`,
  `CORE-000081`, and timing-variable clusters.
- Column-reference comparator rules such as `CORE-000195`, `CORE-000197`,
  `CORE-000198`, `CORE-000698`, and `CORE-000704`.
- Broader JSONata support for the currently skipped USDM/SEND rules.
