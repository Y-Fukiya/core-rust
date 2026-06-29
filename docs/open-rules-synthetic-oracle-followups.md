# Open Rules Synthetic Oracle Follow-Ups

The full upstream score can reach `no_official_oracle = 0` by classifying
missing-official cases with synthetic oracle policy. This is useful for stable
scoreboard accounting, but it is not the same as official oracle conformance.

Latest local full run:

- Total cases: 2298
- Official oracle matches: 2114
- Synthetic oracle matches: 184
- Unverified synthetic oracle matches: 77
- Supported mismatches: 0
- Harness errors: 0

Synthetic reason breakdown:

- `missing official results.csv; synthetic empty positive oracle`: 59 cases
- `missing official results.csv; synthetic candidate issue oracle`: 48 cases
- `missing official results.csv; unverified synthetic candidate oracle`: 73 cases
- `missing official results.csv; unverified synthetic absent-candidate oracle`: 4 cases

## Follow-Up Policy

Treat verified synthetic cases as accounting aids until upstream official
`results.csv` files exist. Treat unverified synthetic cases as warnings only.
They should not be used as proof of official oracle compatibility.

## Recommended Work Queue

1. For cases with candidate skipped rows, implement the missing rule semantics
   or keep them explicitly documented as unverified.
2. For cases with candidate report rows but no official oracle, propose upstream
   `results.csv` additions when the expected result can be justified from the
   rule and fixture.
3. For absent-candidate cases, review upstream discovery shape first. These are
   likely malformed or non-case directories and should not drive engine changes.
4. Keep CI gating on `supported_mismatch > 0` and `harness_error > 0`; keep
   unverified synthetic counts as warnings.

## Unverified Synthetic Inventory

The 77 unverified synthetic cases are grouped below from
`target/open-rules-scoreboard-local-no-oracle-e/scoreboard.json`.

By synthetic reason:

- `missing official results.csv; unverified synthetic candidate oracle`: 73
- `missing official results.csv; unverified synthetic absent-candidate oracle`: 4

By candidate row status:

- `failed`: 280 rows
- `skipped`: 68 rows
- `passed`: 4 rows
- missing candidate report: 4 cases

By skipped reason:

- `oracle_semantics_gap`: 49 rows
- `unsupported_operator`: 11 rows
- `dataset_join_not_supported`: 4 rows
- `evaluation_error`: 2 rows
- `operations_not_supported`: 1 row
- `standard_mismatch`: 1 row

Top rule ids by unverified case count:

- `CORE-000107`: 9
- `CORE-000606`: 4
- `CORE-000609`: 4
- `CORE-000610`: 4
- `CORE-000611`: 4
- `CORE-000638`: 4
- `CORE-000727`: 4
- `CORE-000605`: 3
- `CORE-000607`: 3
- `CORE-000673`: 3
