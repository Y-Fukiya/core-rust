# Open Rules Upstream Filing Plan

Snapshot: `target/open-rules-scoreboard-upstream-v31/scoreboard.json`

The remaining `deferred_oracle_gap_skipped = 55` cases are reviewed oracle or
fixture follow-ups, not core-rust correctness failures. Keep them outside the
supported accuracy gate until upstream evidence changes.

## Current Split

| Family | Cases | Local treatment |
|---|---:|---|
| `official_oracle_fixture_gap` | 51 | Report-only in score output; baseline comparison requires review if this count grows. File upstream reconciliation issues by bundle. |
| `standard_filter_oracle_gap` | 4 | Report-only in score output; baseline comparison requires review if this count grows. Ask upstream to confirm fixture standard metadata or rule applicability. |

## Filing Order

1. `CORE-000698` / `CORE-000704` paired PDVALMIN/PDVALMAX cases.
2. Large official-empty cases: `CORE-000080`, `CORE-000081`.
3. Positive fixture cases with official or candidate issues.
4. Standard applicability cases: `CORE-000217`, `CORE-000478`.
5. Remaining grouped bundles from `open-rules-official-fixture-gap-candidates.md`.

## Ready-To-File Bundles

Use bundles rather than 55 one-off issues. The goal is to ask upstream to
reconcile oracle/data evidence, not to encode Open Rules fixture quirks as
native engine behavior.

| Bundle | Cases | Upstream ask | Local evidence |
|---|---:|---|---|
| Paired PDVAL bound oracle review | 6 | Confirm whether `CORE-000698` expected rows should use PDVALMIN and `CORE-000704` should use PDVALMAX, including paired positive fixtures. | Draft 1 in `open-rules-upstream-issue-drafts.md`; candidate table in `open-rules-official-fixture-gap-candidates.md`. |
| Official-empty large-output timing fixtures | 2 | Confirm whether empty official results for `CORE-000080` and `CORE-000081` are intentional despite thousands of structural candidate rows. | Draft 2 in `open-rules-upstream-issue-drafts.md`. |
| Positive fixtures with issues | 9 | Confirm whether positive fixtures should be clean or whether issue-bearing positive fixtures need explanatory metadata. | Draft 3 in `open-rules-upstream-issue-drafts.md`. |
| Standard applicability mismatch | 4 | Confirm whether the fixture standard metadata or rule applicability should change for `CORE-000217` and `CORE-000478`. | Draft 4 plus `open-rules-standard-filter-gap-candidates.md`. |
| Remaining official fixture/oracle gaps | 34 | File smaller follow-ups by semantic family after the first four bundles are reviewed. | Bundle table in `open-rules-official-fixture-gap-candidates.md`. |

Do not file `CORE-000356` or `CORE-000652` as engine implementation requests
yet. They are currently candidate-skipped because targeted probes found the
committed official rows do not line up with the literal rule/fixture semantics.
After upstream confirms or corrects those expected rows, revisit them as native
engine semantics work.

## Filed Upstream Issues

The first filing wave has been submitted to `cdisc-org/cdisc-open-rules`:

| Bundle | Upstream issue |
|---|---|
| Paired PDVAL bound oracle review | [cdisc-org/cdisc-open-rules#66](https://github.com/cdisc-org/cdisc-open-rules/issues/66) |
| Official-empty large-output timing fixtures | [cdisc-org/cdisc-open-rules#67](https://github.com/cdisc-org/cdisc-open-rules/issues/67) |
| Positive fixtures with issues | [cdisc-org/cdisc-open-rules#68](https://github.com/cdisc-org/cdisc-open-rules/issues/68) |
| Standard applicability mismatch | [cdisc-org/cdisc-open-rules#69](https://github.com/cdisc-org/cdisc-open-rules/issues/69) |

Keep the remaining 34 `official_oracle_fixture_gap` cases local/report-only
until upstream responds to this first wave or a reviewer asks for more granular
follow-ups.

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
