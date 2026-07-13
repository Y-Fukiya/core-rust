# Open Rules Deferred Gap Inventory

Snapshot: `tests/open_rules/upstream-baseline.json`, regenerated from the
pinned full-upstream corpus on 2026-07-14.

This inventory tracks the remaining `deferred_oracle_gap_skipped` cases after
the default-engine upstream run reached:

| Metric | Count |
|---|---:|
| `total_cases` | 2296 |
| `supported_match` | 2060 |
| `supported_mismatch` | 0 |
| `deferred_oracle_gap_mismatch` | 0 |
| `deferred_oracle_gap_skipped` | 54 |
| `skipped_unsupported` | 0 |
| `mixed_skipped_and_issues` | 0 |
| `harness_error` | 0 |
| `no_official_oracle` | 182 |
| `coverage` | 89.72% |
| `native_engine_coverage` | 75.13% |

`deferred_oracle_gap_skipped` is not a correctness failure. These cases are
excluded from supported accuracy and should be reviewed as coverage/oracle
follow-up work.

## Family Summary

| Family | Cases | Treatment |
|---|---:|---|
| `official_oracle_fixture_gap` | 50 | Keep out of the score gate. These official `results.csv` files disagree with the rule or input fixture and should become upstream issue/PR candidates. |
| `standard_filter_oracle_gap` | 4 | Keep out of the score gate. These cases are skipped because the fixture standard/version does not match the rule authority or applicability note. |

## Rule Counts

| Family | Rules |
|---|---|
| `official_oracle_fixture_gap` | `CORE-000698` 2, `CORE-000704` 2, `CORE-000117` 2, `CORE-000172` 2, `CORE-000356` 2, `CORE-000438` 2, `CORE-000648` 2, `CORE-000652` 2, `CORE-000718` 2, `CORE-000014` 1, `CORE-000049` 1, `CORE-000080` 1, `CORE-000081` 1, `CORE-000108` 1, `CORE-000143` 1, `CORE-000184` 1, `CORE-000195` 1, `CORE-000197` 1, `CORE-000198` 1, `CORE-000224` 1, `CORE-000225` 1, `CORE-000252` 1, `CORE-000262` 1, `CORE-000267` 1, `CORE-000268` 1, `CORE-000289` 1, `CORE-000370` 1, `CORE-000454` 1, `CORE-000458` 1, `CORE-000529` 1, `CORE-000542` 1, `CORE-000546` 1, `CORE-000554` 1, `CORE-000569` 1, `CORE-000570` 1, `CORE-000750` 1, `CORE-000770` 1, `CORE-000814` 1, `CORE-000865` 1, `CORE-000884` 1, `CORE-000960` 1 |
| `standard_filter_oracle_gap` | `CORE-000217` 2, `CORE-000478` 2 |

The detailed upstream proposal backlog for the 50
`official_oracle_fixture_gap` cases is tracked in
[`open-rules-official-fixture-gap-candidates.md`](open-rules-official-fixture-gap-candidates.md).
The 4 `standard_filter_oracle_gap` cases are tracked separately in
[`open-rules-standard-filter-gap-candidates.md`](open-rules-standard-filter-gap-candidates.md).

## Deferred Skip Review

The remaining non-fixture-gap cases should not be bulk-returned to supported
scoring. They mix different semantics and the current candidate reports are
skips, not wrong answers:

| Rule | Cases | Review finding |
|---|---:|---|
| `CORE-000217` | 2 | Standard applicability gap: the `/05` fixtures are filtered as SENDIG 3.1, while the rule authority is SDTMIG/TIG. The other eight CORE-000217 cases are already supported matches. |
| `CORE-000478` | 2 | Standard applicability gap: the rule explicitly notes it is not applicable to SENDIG-3.0, while the fixture is SENDIG 3.0. |

`CORE-000642` was returned to supported scoring after narrowing the standard
filter exception. Its SENDIG 3.1 fixture is compatible with the rule's SENDIG
3.1.1 authority, and a targeted upstream subset run produced 2/2
`supported_match` cases.

## Distinct Operation Review

`CORE-000652` is now classified under `official_oracle_fixture_gap`, not as a
native distinct-operation backlog. A targeted external-distinct probe produced
`official=2` and `candidate=1`; the official rows flagged `STDY02-99`, which is
present in DM/EX, while the candidate found `STDY02-101`, which is absent from
DM/EX. Returning it to supported scoring now would create a deferred mismatch,
so this is safer as an upstream oracle/fixture review item.

## Required Value Metadata Review

`CORE-000356` is now classified under `official_oracle_fixture_gap`, not as a
native engine implementation backlog. The official negative oracle lists 28 LB
variables for record 1, including values that are not `Required`/null under the
rule text. Implementing the literal rule semantics would not reproduce that
oracle, so this is currently safer as an upstream oracle/fixture review item.

## Next Actions

1. Keep `official_oracle_fixture_gap` as report-only in the score command
   unless upstream fixture evidence changes. Baseline comparison should require
   review if this inventory grows.
2. Keep `standard_filter_oracle_gap` as report-only in the score command unless
   upstream fixture standard metadata changes. Baseline comparison should
   require review if this inventory grows.
3. Preserve `supported_mismatch = 0`, `skipped_unsupported = 0`,
   `harness_error = 0`, and `deferred_oracle_gap_mismatch = 0` as CI gate
   invariants.
