# Open Rules Upstream Filing Plan

Snapshot: `target/open-rules-scoreboard-upstream-v31/scoreboard.json`

The remaining `deferred_oracle_gap_skipped = 55` cases are reviewed oracle or
fixture follow-ups, not core-rust correctness failures. Keep them outside the
supported accuracy gate until upstream evidence changes.

## Current Split

| Family | Cases | Local treatment |
|---|---:|---|
| `official_oracle_fixture_gap` | 51 | Warning/report-only. File upstream reconciliation issues by bundle. |
| `standard_filter_oracle_gap` | 4 | Warning/report-only. Ask upstream to confirm fixture standard metadata or rule applicability. |

## Filing Order

1. `CORE-000698` / `CORE-000704` paired PDVALMIN/PDVALMAX cases.
2. Large official-empty cases: `CORE-000080`, `CORE-000081`.
3. Positive fixture cases with official or candidate issues.
4. Standard applicability cases: `CORE-000217`, `CORE-000478`.
5. Remaining grouped bundles from `open-rules-official-fixture-gap-candidates.md`.

## Evidence To Attach

For every upstream issue or PR, include:

- rule id and case id
- official issue count
- candidate structural issue count
- `missing_count` and `extra_count`
- `issue_fingerprint_hash`
- short explanation of why the case is excluded from supported accuracy
- whether the request is fixture data correction, `results.csv` correction, or applicability clarification

## Local Gate Policy

Continue failing local or CI regression checks on:

- `supported_mismatch > 0`
- `skipped_unsupported > 0`
- `harness_error > 0`
- `mixed_skipped_and_issues > 0`
- `deferred_oracle_gap_mismatch > 0`

Do not fail on the accepted 55 `deferred_oracle_gap_skipped` cases, but do not
allow that bucket to grow without review.
