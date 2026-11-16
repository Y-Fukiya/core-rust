# Upstream Adoption Audit: 2026-09-25

## Decision

**Candidate only; not accepted.** PRs #149, #151 and #150 have been merged.
The resulting main commit passes both normal CI and the accepted-pin full
upstream regression gate. The latest upstream revision still fails that gate;
do not merge its pin change or regenerate accepted baselines to hide failures.

The candidate branch changes only the lock and this audit record/inventory.
It does not change execution semantics, scorer policy, gap manifests, the four
accepted baselines, or the frozen pilot expectations. Its ordinary pilot pin
consistency check is intentionally unresolved until the source changes below
have been reviewed. A successful observe job means that audit artifacts were
produced, not that the candidate has passed conformance or acceptance.

| Input | Revision / evidence |
|---|---|
| Merged core-rust main | `fa99a2be857a7f40c0c9bf8d4b2dc0c2ec27fcf0` |
| Main Rust/Python CI | [36102965163, success](https://github.com/Y-Fukiya/core-rust/actions/runs/36102965163) |
| Accepted upstream | `7f7fae49376b3d023563ebb6c36a3b392d6e649f` |
| Accepted full run | [36103056372, observe and regression success](https://github.com/Y-Fukiya/core-rust/actions/runs/36103056372) |
| Candidate core-rust code | `97bd195`, same implementation as merged main, only the upstream lock changed |
| Candidate upstream | `d3a9dfb376c8953eac1f917e307d46b56958c5dc`, September 18 |
| Candidate full run | [36103118800, observe success, regression failure](https://github.com/Y-Fukiya/core-rust/actions/runs/36103118800) |

Both full runs verified their expected and observed upstream SHA with
`--strict-lock`. Within each run, strict scoring reused the default candidate
reports and the delta input-correspondence checks passed. These are two
different corpora, so the cross-pin differences below are not automatically
engine regressions on unchanged inputs.

## Fresh Full-Corpus Results

| Metric | Accepted default | Candidate default | Accepted strict | Candidate strict |
|---|---:|---:|---:|---:|
| Total cases | 2296 | 2293 | 2296 | 2293 |
| Supported match | 2060 | 2142 | 1207 | 1287 |
| Supported mismatch | 0 | 62 | 899 | 954 |
| Deferred oracle-gap mismatch | 0 | 21 | 0 | 0 |
| Deferred oracle-gap skipped | 54 | 24 | 0 | 0 |
| Skipped unsupported | 0 | 34 | 8 | 42 |
| No official oracle | 182 | 10 | 182 | 10 |
| Mixed skipped and issues | 0 | 0 | 0 | 0 |
| Harness error | 0 | 0 | 0 | 0 |

Candidate baseline comparison: **125 regression entries, 134 improvement
entries, 16 review-required entries**, exit 1. These are comparator entries,
not unique failing cases. Default contains 83 mismatched cases (62 supported
and 21 deferred), plus 34 unsupported cases. The 24 deferred skips and ten
missing-oracle cases also require review, not automatic acceptance.

The default-to-strict delta contains 855 supported-match to supported-mismatch
transitions. Strict scoring is a separate audit, not the current release gate.
Default normalization counts and strict deltas have different denominators and
must not be treated as interchangeable evidence of native conformance.

The [case inventory](open-rules-upstream-update-20260925-cases.csv) lists all
151 candidate cases outside `supported_match`, their previous bucket, counts,
and missing/extra counts. It is an audit inventory, **not an accepted baseline
or exception allowlist**. Absent counts are empty rather than invented zeros.

## Cross-Pin Transitions

| Accepted bucket -> candidate bucket | Cases |
|---|---:|
| Deferred skipped -> supported match | 33 |
| No official oracle -> supported match | 110 |
| No official oracle -> supported mismatch | 23 |
| No official oracle -> skipped unsupported | 34 |
| No official oracle -> deferred skipped | 3 |
| Supported match -> supported mismatch | 39 |
| Supported match -> deferred mismatch | 20 |
| Supported match -> deferred skipped | 1 |

Six accepted case keys disappear: CORE-000039 negative/02, CORE-000213
negative/03, CORE-000220 negative/02, CORE-000224 negative/02, CORE-000225
negative/02 and CORE-000272 negative/04. Three appear: CORE-000039 positive/02,
CORE-000674 negative/02 and negative/03. These are independent lists, not
approved rename mappings. The new CORE-000039 positive/02 is a deferred
mismatch, official zero versus candidate six.

## Implementation And Identity Work

1. External/grouped distinct: **31 unsupported cases**, CORE-000208/209/210/296
   and CORE-000605/606/607/609/610/611/727. Implement and test these semantics
   separately from the pin/baseline acceptance change.
2. `present_on_multiple_rows_within`: **two unsupported cases**, CORE-000363.
   The rule uses subject-grouped repeated DSDECOD, `min_date`, and a DM join;
   fixing only operator recognition is not proof of rule conformance.
3. RELREC join: **one unsupported case**, CORE-000901. Its other case still
   lacks an official oracle; do not count it as a match.
4. Prefix regex CORE-000107 now executes all nine cases: four positive matches
   and five negative mismatches. Previously all nine were unsupported on the
   September 2 proposal. Negative official/candidate counts remain
   9/40068, 9/18, 9/27, 9/40068 and 45/135. Execution coverage improved, but
   reporting/scope semantics are unresolved.
5. Of the 83 default mismatches, 26 have equal issue counts (22 supported,
   four deferred) and 57 differ in count. Equal counts are only a triage hint,
   not permission to discard locators or normalize toward the official answer.

The [September 9 audit](open-rules-upstream-update-20260909.md) remains the
historical comparison for the September 2 proposal. Its totals must not be
presented as the current candidate result.

## Pilot Source Review

All 12 rule directories in `tests/open_rules/pilot-review.json` were compared
between the accepted and candidate official archives. There are no added or
removed files within those directories. Input CSVs, environment files, and
parsed `Check`, `Scope`, `Operations`, `Match Datasets`, `Rule Type` and
`Sensitivity` are unchanged. Every `rule.yml` changes in description, outcome
and/or authority metadata; applicability/citation changes still need review.

The existing frozen `(Dataset, Record, Variable)` expectations agree with
22 of the 24 candidate official fixtures. Two negative fixtures change their
identity convention:

| Rule | Accepted official / frozen expectation | Candidate official | Candidate engine |
|---|---|---|---|
| CORE-000042 | TT, row 1, variable TT | STUDY, no row, variable TT | TT, row 1, variable TT |
| CORE-000043 | TP, row 1, variable TP | STUDY, no row, variable TP | TP, row 1, variable TP |

Both are Domain Presence Check rules with Dataset sensitivity. Review the
study-level identity contract and its impact on the accepted corpus before
changing report output. Do not replace the frozen expectations or weaken the
pilot checker just to allow the new SHA. CORE-000004 and CORE-000020 also have
changed result CSV bytes, but their structural projection is unchanged.
The pilot remains a Codex rule/fixture review with **human approval pending**.
Running `tests/test_open_rules_pilot.py` on the candidate gives 20 passes and
one failure: the committed pilot SHA does not equal the proposed lock SHA.
This expected rejection is retained, not suppressed; candidate CI is not green.

## Previously Filed Upstream Issues

Issues #66-#69 have already received answers; they are not waiting for a first
response. On this candidate, CORE-000698/704 and CORE-000217 still match their
cases. Existing policy labels must be rechecked against the updated fixtures:

- CORE-000080 negative/01: official 4, candidate 6360.
- CORE-000081 negative/01 and /02: official 2/2, candidate 3232/136.
- CORE-000648 positive/01: official 0, candidate 2.
- CORE-000478: both cases remain skipped for applicability.

These are still labeled deferred skips by existing policy. That label is not
fresh evidence that upstream is wrong; do not carry the original empty-oracle
explanation forward after upstream populated the results.

## Promotion Checklist

- [x] Merge #149, #151 and #150; confirm main Rust/Python CI.
- [x] Confirm accepted-pin full regression passes on the merged code.
- [x] Run latest candidate default, strict and delta; retain failure evidence.
- [x] Inventory all non-matching candidate cases and check the 12-rule pilot sources.
- [ ] Resolve/review the 83 mismatches and 34 unsupported cases without hiding them.
- [ ] Reassess deferred/missing-oracle cases and removed/added case identities.
- [ ] Review authority changes and study-level identity for the frozen pilot.
- [ ] Update full/curated baselines and pilot source hashes only after acceptance review.
- [ ] Pass normal CI and full regression on the final candidate, then merge its pin.

## Artifact Integrity And Reproduction

Download `open-rules-upstream-scoreboard` from each run above. Each contains
default and strict JSON/Markdown, delta JSON/Markdown and the candidate run
summary. The candidate regression log contains the baseline comparison failure.
Archive the artifacts before Actions retention expires; use the linked run
IDs rather than relying on temporary local directories.

SHA-256 of downloaded evidence:

| Run / file | SHA-256 |
|---|---|
| Accepted default scoreboard | `b8dd1dd72aacb963d99e03d339cbce21dd25b3f03af8dcf9c9cdf66e8f60cf95` |
| Accepted strict scoreboard | `c83ff25a448b03bd9c4f235da04420b57a0eb1cd60622a242e4146cf87444c7c` |
| Candidate default scoreboard | `ba5b29e9f833bcddafa3eed9779a0ac66cb601906aad0efbd1afc11522d005b0` |
| Candidate strict scoreboard | `3d2b7adb4b48173f6f52f4c52d48a1968b5ac1fda4b1516cfb8b6d62eeab7dcd` |
| Candidate delta JSON | `360710a3903641d708cb252067f97a939da7207aff32a128c89ec8eea07e9ea2` |

Re-run `open-rules-upstream.yml` at the reviewed code ref, or follow the
individual run-score / score / score-delta / baseline commands in the September
9 audit with the candidate SHA above. Keep default and strict nonzero scoring
statuses visible and never substitute the proposed scoreboard for the accepted
baseline merely to turn the regression job green.
