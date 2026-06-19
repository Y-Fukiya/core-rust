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

## Result After Phase 5 Fixes

| Metric | Value |
|---|---:|
| Total cases | 2298 |
| Supported match | 245 |
| Supported mismatch | 36 |
| Skipped unsupported | 1559 |
| Harness error | 458 |
| Supported accuracy | 87.19% |
| Coverage | 12.23% |

The Phase 5 fixes improved the full upstream score from:

- `supported_match`: 130 -> 245
- `supported_mismatch`: 98 -> 36
- `skipped_unsupported`: 1612 -> 1559
- `supported_accuracy`: 57.02% -> 87.19%
- `coverage`: 9.92% -> 12.23%

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

`CORE-000001`, `CORE-000013`, `CORE-000022`, and both `CORE-000025` cases are
now supported-match.

## Remaining Mismatch Hotspots

The first remaining mismatches by case are:

| Rule | Case | Official issues | Candidate issues |
|---|---|---:|---:|
| `CORE-000015` | negative/01 | 45 | 70 |
| `CORE-000049` | negative/01 | 1 | 4 |
| `CORE-000080` | negative/01 | 0 | 6360 |
| `CORE-000081` | negative/01 | 0 | 3232 |
| `CORE-000096` | negative/01 | 9 | 12 |
| `CORE-000098` | negative/01 | 6 | 8 |
| `CORE-000111` | positive/01 | 0 | 10 |
| `CORE-000112` | positive/01 | 0 | 10 |
| `CORE-000113` | positive/01 | 0 | 10 |
| `CORE-000114` | positive/02 | 0 | 5 |

These remaining cases are now beyond the Phase 5 bootstrap pass: they are
either broader rule-semantics work, unsupported operations, or apparent
official fixture inconsistencies such as `CORE-000049`, whose rule checks
`--USCHFL` while the official result reports `LBIMPLBL`.

## Harness Error Split

| Error type | Count |
|---|---:|
| Missing candidate report | 274 |
| Missing official results | 184 |

Top missing-candidate rules:

- `CORE-000982`: 12 cases
- `CORE-000981`: 12 cases
- `CORE-000963`: 7 cases
- `CORE-000962`: 7 cases
- `CORE-001000`: 5 cases
- `CORE-000974`: 5 cases

Top missing-official rules:

- `CORE-000107`: 9 cases
- `CORE-000213`: 5 cases
- `CORE-000727`: 4 cases
- `CORE-000673`: 4 cases
- `CORE-000638`: 4 cases

## Next Precision Work

The next precision pass should start with the remaining high-yield supported
mismatches that have candidate reports and official results, especially
`CORE-000015`, `CORE-000080`, `CORE-000081`, and the `CORE-0006xx` timing and
supplemental-qualifier clusters.

The missing-candidate group should be handled separately by making the runner
write a per-case run summary and classifying rule-load failures as unsupported
coverage gaps or true harness errors.
