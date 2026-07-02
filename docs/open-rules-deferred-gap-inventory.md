# Open Rules Deferred Gap Inventory

Snapshot: `target/open-rules-scoreboard-upstream-v26/scoreboard.json`

This inventory tracks the remaining `deferred_oracle_gap_skipped` cases after
the default-engine upstream run reached:

| Metric | Count |
|---|---:|
| `total_cases` | 2296 |
| `supported_match` | 2059 |
| `supported_mismatch` | 0 |
| `deferred_oracle_gap_mismatch` | 0 |
| `deferred_oracle_gap_skipped` | 55 |
| `skipped_unsupported` | 0 |
| `mixed_skipped_and_issues` | 0 |
| `harness_error` | 0 |
| `no_official_oracle` | 182 |
| `coverage` | 89.68% |
| `native_engine_coverage` | 75.26% |

`deferred_oracle_gap_skipped` is not a correctness failure. These cases are
excluded from supported accuracy and should be reviewed as coverage/oracle
follow-up work.

## Family Summary

| Family | Cases | Treatment |
|---|---:|---|
| `official_oracle_fixture_gap` | 47 | Keep out of the score gate. These official `results.csv` files disagree with the rule or input fixture and should become upstream issue/PR candidates. |
| `defer_positive_zero_probe` | 6 | Keep as review backlog. These are positive/zero official-result probes where native semantics need stronger evidence before returning them to supported scoring. |
| `required_value_metadata` | 2 | Keep out of the score gate. The observed `CORE-000356` oracle does not match the literal Required/null rule semantics. |

## Rule Counts

| Family | Rules |
|---|---|
| `official_oracle_fixture_gap` | `CORE-000698` 3, `CORE-000704` 3, `CORE-000117` 2, `CORE-000172` 2, `CORE-000438` 2, `CORE-000648` 2, `CORE-000718` 2, `CORE-000014` 1, `CORE-000049` 1, `CORE-000080` 1, `CORE-000081` 1, `CORE-000108` 1, `CORE-000143` 1, `CORE-000184` 1, `CORE-000195` 1, `CORE-000197` 1, `CORE-000198` 1, `CORE-000224` 1, `CORE-000225` 1, `CORE-000252` 1, `CORE-000262` 1, `CORE-000267` 1, `CORE-000268` 1, `CORE-000289` 1, `CORE-000370` 1, `CORE-000454` 1, `CORE-000458` 1, `CORE-000529` 1, `CORE-000542` 1, `CORE-000546` 1, `CORE-000554` 1, `CORE-000569` 1, `CORE-000570` 1, `CORE-000750` 1, `CORE-000770` 1, `CORE-000814` 1, `CORE-000865` 1, `CORE-000960` 1 |
| `defer_positive_zero_probe` | `CORE-000217` 2, `CORE-000478` 2, `CORE-000652` 2 |
| `required_value_metadata` | `CORE-000356` 2 |

The detailed upstream proposal backlog for the 47
`official_oracle_fixture_gap` cases is tracked in
[`open-rules-official-fixture-gap-candidates.md`](open-rules-official-fixture-gap-candidates.md).

## Positive-Zero Probe Review

The remaining `defer_positive_zero_probe` cases should not be bulk-returned to
supported scoring. They mix different semantics and the current candidate
reports are skips, not wrong answers:

| Rule | Cases | Review finding |
|---|---:|---|
| `CORE-000217` | 2 | EC dose text fallback with `empty`/`not_exists` branches. Needs targeted empty/null semantics before scoring. |
| `CORE-000478` | 2 | Placeholder timing variables (`--STINT`, `--ENINT`, `--TPTREF`) with SEND scope. Needs placeholder expansion and missing-column handling review. |
| `CORE-000652` | 2 | GV `USUBJID` containment against DM/EX distinct operations. Needs distinct operation evidence plus SEND control-subject semantics review. |

`CORE-000642` was returned to supported scoring after narrowing the standard
filter exception. Its SENDIG 3.1 fixture is compatible with the rule's SENDIG
3.1.1 authority, and a targeted upstream subset run produced 2/2
`supported_match` cases.

## Required Value Metadata Review

`CORE-000356` remains excluded from supported scoring. The official negative
oracle lists 28 LB variables for record 1, including values that are not
`Required`/null under the rule text. Implementing the literal rule semantics
would not reproduce that oracle, so this is currently safer as an oracle/fixture
review item than as a native engine change.

## Next Actions

1. Keep `official_oracle_fixture_gap` as warning/report-only unless upstream
   fixture evidence changes.
2. Split `defer_positive_zero_probe` by rule semantics before returning any
   case to supported scoring.
3. Keep `CORE-000356` out of supported scoring unless upstream oracle/data
   evidence changes.
4. Preserve `supported_mismatch = 0`, `skipped_unsupported = 0`,
   `harness_error = 0`, and `deferred_oracle_gap_mismatch = 0` as CI gate
   invariants.
