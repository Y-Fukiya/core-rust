# Open Rules Deferred Gap Inventory

Snapshot: `target/open-rules-scoreboard-upstream-v25/scoreboard.json`

This inventory tracks the remaining `deferred_oracle_gap_skipped` cases after
the default-engine upstream run reached:

| Metric | Count |
|---|---:|
| `total_cases` | 2296 |
| `supported_match` | 2057 |
| `supported_mismatch` | 0 |
| `deferred_oracle_gap_mismatch` | 0 |
| `deferred_oracle_gap_skipped` | 57 |
| `skipped_unsupported` | 0 |
| `mixed_skipped_and_issues` | 0 |
| `harness_error` | 0 |
| `no_official_oracle` | 182 |
| `coverage` | 89.59% |
| `native_engine_coverage` | 75.17% |

`deferred_oracle_gap_skipped` is not a correctness failure. These cases are
excluded from supported accuracy and should be reviewed as coverage/oracle
follow-up work.

## Family Summary

| Family | Cases | Treatment |
|---|---:|---|
| `official_oracle_fixture_gap` | 47 | Keep out of the score gate. These official `results.csv` files disagree with the rule or input fixture and should become upstream issue/PR candidates. |
| `defer_positive_zero_probe` | 8 | Keep as review backlog. These are positive/zero official-result probes where native semantics need stronger evidence before returning them to supported scoring. |
| `required_value_metadata` | 2 | Engine-semantics candidate. Required-value metadata can be implemented and verified separately. |

## Rule Counts

| Family | Rules |
|---|---|
| `official_oracle_fixture_gap` | `CORE-000698` 3, `CORE-000704` 3, `CORE-000117` 2, `CORE-000172` 2, `CORE-000438` 2, `CORE-000648` 2, `CORE-000718` 2, `CORE-000014` 1, `CORE-000049` 1, `CORE-000080` 1, `CORE-000081` 1, `CORE-000108` 1, `CORE-000143` 1, `CORE-000184` 1, `CORE-000195` 1, `CORE-000197` 1, `CORE-000198` 1, `CORE-000224` 1, `CORE-000225` 1, `CORE-000252` 1, `CORE-000262` 1, `CORE-000267` 1, `CORE-000268` 1, `CORE-000289` 1, `CORE-000370` 1, `CORE-000454` 1, `CORE-000458` 1, `CORE-000529` 1, `CORE-000542` 1, `CORE-000546` 1, `CORE-000554` 1, `CORE-000569` 1, `CORE-000570` 1, `CORE-000750` 1, `CORE-000770` 1, `CORE-000814` 1, `CORE-000865` 1, `CORE-000960` 1 |
| `defer_positive_zero_probe` | `CORE-000217` 2, `CORE-000478` 2, `CORE-000642` 2, `CORE-000652` 2 |
| `required_value_metadata` | `CORE-000356` 2 |

## Next Actions

1. Keep `official_oracle_fixture_gap` as warning/report-only unless upstream
   fixture evidence changes.
2. Split `defer_positive_zero_probe` by rule semantics before returning any
   case to supported scoring.
3. Treat `CORE-000356` required-value metadata as the next native engine
   semantics candidate.
4. Preserve `supported_mismatch = 0`, `skipped_unsupported = 0`,
   `harness_error = 0`, and `deferred_oracle_gap_mismatch = 0` as CI gate
   invariants.
