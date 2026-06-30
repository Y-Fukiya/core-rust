# Open Rules Upstream Baseline

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

## Current Committed Regression Baseline

These numbers describe the committed `tests/open_rules/upstream-baseline.json`.
They are a regression baseline for the current engine and oracle harness, not a
claim of full CDISC Open Rules conformance.

| Metric | Value |
|---|---:|
| Total cases | 2298 |
| Supported match | 1793 |
| Supported mismatch | 254 |
| Skipped unsupported | 63 |
| Mixed skipped/issues | 2 |
| No official oracle | 186 |
| Harness error | 0 |
| Supported accuracy | 87.59% |
| Aggregate coverage | 89.08% |
| Native engine coverage | 75.02% |
| Rule-id hand-port coverage | 14.06% |

The aggregate coverage includes both native engine execution and explicitly
tracked Open Rules rule-id hand ports. Read it together with
`native_engine_coverage`; a hand-port match is useful regression coverage, but
it is not evidence that generic engine semantics fully cover that rule family.

The baseline intentionally preserves known gaps:

- `supported_mismatch` cases remain correctness gaps to reduce.
- `no_official_oracle` cases are excluded from supported accuracy because the
  upstream case has no official `results.csv`.
- `skipped_unsupported` cases are coverage gaps, not correctness matches.
- `mixed_skipped_and_issues` is a failing bucket because skipped rows and issue
  rows in one candidate report cannot be treated as green.

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

There are 254 `supported_mismatch` cases in the committed upstream baseline.
They are the next source of native engine precision work. Baseline comparison
is intended to prevent these known gaps from increasing and to flag
same-bucket issue-count regressions even when the bucket name stays
`supported_mismatch`.

## Harness Error Split

| Error type | Count |
|---|---:|
| Missing candidate report | 0 |
| Missing official results classified as `no_official_oracle` | 186 |
| Harness error | 0 |

## Skipped Unsupported Split

These are case counts after excluding `no_official_oracle` cases.

| Skipped reason | Cases |
|---|---:|
| `evaluation_error` | 31 |
| `dataset_join_not_supported` | 14 |
| `operations_not_supported` | 10 |
| `oracle_semantics_gap` | 6 |
| `unsupported_rule_type` | 2 |

## Next Precision Work

The next precision pass should reduce `supported_mismatch` while keeping
`harness_error = 0`, starting with:

- `dataset_join_not_supported` first because it is a small bucket at 14
  cases and can be isolated behind join-specific fixtures.
- `evaluation_error` after adding regression fixtures for the representative
  evaluation failure classes.
- `operations_not_supported`, `oracle_semantics_gap`, and
  `unsupported_rule_type` last, because those buckets contain broader
  rule-model and oracle-semantics decisions rather than one small harness gap.
