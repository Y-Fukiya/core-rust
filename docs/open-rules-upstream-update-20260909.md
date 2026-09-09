# Upstream Update Audit: 2026-09-09

## Decision

Do not promote the new upstream pin yet. Keep the accepted lock and all four
committed baselines unchanged. The audit infrastructure fixes are isolated in
`1298e37`; this update assessment changes neither engine semantics nor gap
manifests. An upstream issue being closed is evidence to retest, not permission
to relabel a candidate mismatch or overwrite the regression baseline.

| Input | Commit |
|---|---|
| core-rust code under test | `1298e37` |
| Accepted upstream | `7f7fae49376b3d023563ebb6c36a3b392d6e649f` |
| Proposed upstream, latest observed on September 9 | `1fb7b81e40bdb6632375761c561fabd29676a477` (September 2) |

Both full runs used the same core-rust binary and checked the corresponding
SHA with `--strict-lock`. The proposed SHA was set only for its isolated audit;
the tracked lock was restored to the accepted SHA afterwards. No rule ids were
removed from manifests to improve scores. Both checkouts executed every
discovered case without a harness error.

## Results

| Metric | Accepted pin, fresh run | Proposed pin, default | Proposed pin, strict |
|---|---:|---:|---:|
| Total cases | 2296 | 2293 | 2293 |
| Supported match | 2060 | 2138 | 1283 |
| Supported mismatch | 0 | 57 | 949 |
| Deferred oracle-gap mismatch | 0 | 21 | 0 |
| Deferred oracle-gap skipped | 54 | 24 | 0 |
| Skipped unsupported | 0 | 43 | 51 |
| No official oracle | 182 | 10 | 10 |
| Mixed skipped and issues | 0 | 0 | 0 |
| Harness error | 0 | 0 | 0 |

The proposed default summary's 24 deferred skips comprise 21 official-fixture
labels, two standard-filter labels and one candidate skip. These are labels
from the existing policy, not renewed evidence that the updated upstream is
wrong. Reassess them against the new rules and fixtures before promotion.

The fresh accepted-pin baseline comparison passes. Comparing the proposed
default scoreboard with the accepted baseline returns **129 regression entries,
130 improvement entries and 16 review-required entries**, and exits nonzero.
Entries are comparator findings, not counts of unique failing cases.
Because the inputs changed, these differences do not by themselves establish
engine regressions on unchanged inputs.

Strict scoring reuses the exact same candidate reports as default scoring.
The default-vs-strict delta passes its input correspondence checks and is an
audit artifact, not a substitute for the failing regression comparison.

## What Changed In Coverage

- 106 previously oracle-less cases now match.
- 43 previously oracle-less cases now expose unsupported execution.
- 18 previously oracle-less cases now expose supported mismatches.
- Three previously oracle-less cases become deferred skips.
- 33 previously deferred skips now match.
- 39 previously supported matches become supported mismatches.
- 20 previously supported matches become deferred mismatches.
- One previously supported match becomes a deferred skip.

Case identity changes must also be reviewed, not silently dropped:

| Removed case | Added case |
|---|---|
| CORE-000039 negative/02 | CORE-000039 positive/02 |
| CORE-000213 negative/03 | CORE-000674 negative/02 |
| CORE-000220 negative/02 | CORE-000674 negative/03 |
| CORE-000224 negative/02 | |
| CORE-000225 negative/02 | |
| CORE-000272 negative/04 | |

The two columns are independent lists, not proposed rename mappings.

## Filed Issues: Answers And Local Retests

| Issue | Upstream response | Local proposed-pin result |
|---|---|---|
| [#66](https://github.com/cdisc-org/cdisc-open-rules/issues/66#issuecomment-5399605422) | Closed August 24; corrected and validated. | CORE-000698 and CORE-000704: all 10 cases match. The four previously deferred negative cases now match. |
| [#67](https://github.com/cdisc-org/cdisc-open-rules/issues/67#issuecomment-5399557149) | Closed August 24; negative results populated. | The original empty-oracle problem is fixed, but CORE-000080 negative/01 is official 4 vs candidate 6360; CORE-000081 negative/01 is 2 vs 3232 and negative/02 is 2 vs 136. Review candidate timing/scope and output identity against the updated rules. Do not carry the old empty-oracle explanation forward. |
| [#68](https://github.com/cdisc-org/cdisc-open-rules/issues/68#issuecomment-5428248476) | Closed August 26; listed fixtures fixed, new structural evidence requested for any remaining problem. | Eight of the nine positive cases in the original draft match. CORE-000648 positive/01 remains official 0 vs candidate 2. Investigate those two findings before proposing a follow-up. |
| [#69](https://github.com/cdisc-org/cdisc-open-rules/issues/69#issuecomment-5399783823) | Closed August 24; resolved and validated. | CORE-000217 now matches all 10 cases, including both /05 cases. CORE-000478 still skips both cases: the inspected fixture environment remains SENDIG 3.0. Reconcile applicability evidence before changing the filter. |

These results supersede "waiting for the first upstream response" as an action
status. Historical filing drafts and the accepted-pin inventories remain valid
records of what was originally reported.

## Ordered Follow-Up Work

1. Review the 39 supported-to-supported mismatches and 20 supported-to-deferred
   mismatches. Separate changed expected output/locator conventions from rule
   semantics changes; add focused regression tests before changing execution.
2. Implement the newly observable unsupported families in separate engine
   changes: **31 external/grouped distinct cases** (CORE-000208/209/210/296,
   CORE-000605/606/607/609/610/611/727), **11 operator cases** (CORE-000107,
   prefix regex operators; CORE-000363, `present_on_multiple_rows_within`),
   and **one RELREC join case** (CORE-000901). These were previously missing
   official oracles, not previously passing execution coverage.
3. Reassess CORE-000080/081, CORE-000648 and CORE-000478 using the responses
   above. Neither a closed upstream issue nor a local policy label proves that
   the remaining difference is an upstream bug.
4. Review the six removed and three added case keys. Once the blocking families
   are resolved, update the pin, full baseline and both curated baselines in a
   dedicated acceptance change. Retain default, strict and delta artifacts.

## Reproduction And Evidence

Use a disposable core-rust worktree at the code commit above and a clean
upstream checkout. Set that worktree's `tests/open_rules/upstream.lock` to the
proposed SHA only while testing the proposal. Keep the committed accepted
baseline intact for the comparison below.

```sh
cargo run -p xtask --locked -- open-rules run-score \
  --open-rules-root ../cdisc-open-rules \
  --core-rs-results-root target/upstream-update/candidate \
  --out target/upstream-update/default --strict-lock
cargo run -p xtask --locked -- open-rules score \
  --open-rules-root ../cdisc-open-rules \
  --core-rs-results-root target/upstream-update/candidate \
  --out target/upstream-update/strict --strict-scoring
cargo run -p xtask --locked -- open-rules score-delta \
  --default-scoreboard target/upstream-update/default/scoreboard.json \
  --strict-scoreboard target/upstream-update/strict/scoreboard.json \
  --out target/upstream-update/delta
cargo run -p xtask --locked -- open-rules baseline \
  --scoreboard target/upstream-update/default/scoreboard.json \
  --baseline tests/open_rules/upstream-baseline.json
```

Default and strict scoring intentionally exit nonzero for this proposal; run
the commands individually and retain their scoreboards. Archive the candidate
run summary, both scoreboards and summaries, delta JSON/Markdown, and the
baseline comparison output. Re-run the accepted pin separately as a control.

On the macOS audit host, CORE-000749 positive/01 has a case-insensitive path
collision between `Results/results.csv` and `results/results.csv`. Both Git
entries refer to the same blob, `a13018c5f063f58a0f793c5939e5388f2aba7345`, and
the checkout was clean. No fixture content was edited to accommodate it.
