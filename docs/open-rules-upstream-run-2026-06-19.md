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

## Result After Phase 5 First Fixes

| Metric | Value |
|---|---:|
| Total cases | 2298 |
| Supported match | 147 |
| Supported mismatch | 81 |
| Skipped unsupported | 1612 |
| Harness error | 458 |
| Supported accuracy | 64.47% |
| Coverage | 9.92% |

The first Phase 5 fixes improved the full upstream score from:

- `supported_match`: 130 -> 147
- `supported_mismatch`: 98 -> 81
- `supported_accuracy`: 57.02% -> 64.47%

## Fixes Applied

- Preserve YAML `value: N` and related CDISC literal values as strings before
  YAML boolean coercion can convert them to booleans.
- Expand multi-variable candidate report rows into variable-level issue keys,
  matching official `results.csv` rows.
- When official `results.csv` lacks subject or sequence columns, remove those
  candidate-only display fields from primary comparison keys.

`CORE-000001` is now supported-match for both positive and negative cases.

## Remaining Mismatch Hotspots

The first remaining mismatches by case are:

| Rule | Case | Official issues | Candidate issues |
|---|---|---:|---:|
| `CORE-000013` | negative/01 | 13 | 1 |
| `CORE-000013` | negative/02 | 13 | 1 |
| `CORE-000015` | negative/01 | 45 | 0 |
| `CORE-000022` | negative/01 | 121 | 99 |
| `CORE-000025` | negative/01 | 6 | 3 |
| `CORE-000025` | positive/01 | 0 | 3 |
| `CORE-000029` | negative/01 | 2 | 0 |
| `CORE-000029` | negative/02 | 2 | 0 |
| `CORE-000030` | negative/01 | 20 | 0 |
| `CORE-000030` | negative/02 | 34 | 0 |

These are now proper Phase 5 engine/rule-semantics work rather than harness
bootstrap work.

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

The next Phase 5 pass should start with the high-yield supported mismatches
that have candidate reports and official results, especially `CORE-000013`,
`CORE-000015`, `CORE-000022`, and `CORE-000025`.

The missing-candidate group should be handled separately by making the runner
write a per-case run summary and classifying rule-load failures as unsupported
coverage gaps or true harness errors.
