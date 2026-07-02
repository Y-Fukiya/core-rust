# Open Rules Official Fixture Gap Candidates

Snapshot: `target/open-rules-scoreboard-upstream-v29/scoreboard.json`

These cases are currently classified as `official_oracle_fixture_gap` and are
excluded from supported accuracy. They should not be converted into native
engine compatibility hacks without upstream evidence, because the observed
official oracle and candidate output disagree in ways that may reflect fixture,
oracle, or rule-text drift.

## Summary

| Metric | Count |
|---|---:|
| Candidate cases | 51 |
| Distinct rules | 41 |
| Supported gate impact | 0 |
| Recommended treatment | Upstream issue/PR evidence backlog |

## Triage Buckets

| Bucket | Meaning |
|---|---|
| `official-empty` | Official `results.csv` is empty while the candidate reports structural issues from the fixture/rule. |
| `candidate-empty` | Official `results.csv` reports issues while the candidate has no structural issues. |
| `candidate-skip` | Candidate skips because native semantics are intentionally not implemented, and the official oracle/data need reconciliation before implementing them. |
| `positive-fixture` | A positive fixture has official or candidate issues; treat as an oracle/data applicability question first. |
| `cardinality` | Both sides report issues but the counts differ. |
| `identity` | Counts are equal but structural identity differs. |

## Candidate Cases

| Rule | Case | Official | Candidate | Missing | Extra | Fingerprint | Triage |
|---|---|---:|---:|---:|---:|---|---|
| `CORE-000014` | positive/02 | 3 | 0 | 3 | 0 | `53fba3b163dbc15c` | `positive-fixture`; official reports SUOCCUR issues although fixture value is blank. |
| `CORE-000049` | negative/01 | 1 | 4 | 1 | 4 | `9182924229765a33` | `cardinality`; official uses LBIMPLBL while rule checks --USCHFL. |
| `CORE-000080` | negative/01 | 0 | 6360 | 0 | 6360 | `c9cc4e7f39f2fa67` | `official-empty`; fixture contains --TPTREF without timing reference columns. |
| `CORE-000081` | negative/01 | 0 | 3232 | 0 | 3232 | `7d8f17c321ea1553` | `official-empty`; fixture contains --STAT without --PRESP. |
| `CORE-000108` | negative/02 | 0 | 2 | 0 | 2 | `28d6cf25ded95ca5` | `official-empty`; DD contains subject with blank DM.DTHFL. |
| `CORE-000117` | negative/01 | 17 | 10 | 7 | 0 | `eb94259963cb680f` | `cardinality`; context variables and typo do not align with rule output variables. |
| `CORE-000117` | negative/02 | 53 | 34 | 19 | 0 | `ac84594258661f22` | `cardinality`; context variables and typo do not align with rule output variables. |
| `CORE-000143` | positive/02 | 0 | 1 | 0 | 1 | `c205443c8aac7669` | `positive-fixture`; positive fixture contains ETCD longer than eight characters. |
| `CORE-000172` | negative/05 | 0 | 1 | 0 | 1 | `d840ed84ea1a0404` | `official-empty`; STUDYID value in AE data disagrees with official result. |
| `CORE-000172` | positive/05 | 1 | 0 | 1 | 0 | `1dacf7ea9cc71476` | `positive-fixture`; STUDYID value in AE data disagrees with official result. |
| `CORE-000184` | negative/02 | 12 | 22 | 0 | 10 | `4103928b23ed5932` | `cardinality`; official omits MH not-unique relationship rows. |
| `CORE-000195` | negative/01 | 28 | 30 | 0 | 2 | `46307247053a6dcb` | `cardinality`; official omits one AESCAT equals AEDECOD fixture row. |
| `CORE-000197` | negative/02 | 45 | 51 | 0 | 6 | `e3ccfc92fa326421` | `cardinality`; official omits rows where MHCAT equals MHBODSYS. |
| `CORE-000198` | negative/02 | 18 | 51 | 0 | 33 | `c83e311d2d86e0ac` | `cardinality`; official omits rows where MHSCAT equals MHBODSYS. |
| `CORE-000224` | negative/02 | 1 | 0 | 1 | 0 | `b49eca090dad85b7` | `candidate-empty`; official reports DTHFL for ACTARM/ARMNRS rule. |
| `CORE-000225` | negative/01 | 8 | 6 | 4 | 2 | `1321b3e592a05c6a` | `cardinality`; official record values appear shifted for --REASND/--STAT. |
| `CORE-000252` | negative/02 | 0 | 2 | 0 | 2 | `ff3ce9cce8b1c50f` | `official-empty`; DS contains DEATH for a subject whose DM.DTHFL is blank. |
| `CORE-000262` | negative/01 | 0 | 4 | 0 | 4 | `636a97ad39a48d7f` | `official-empty`; fixture has empty DM.RFSTDTC and populated CMSTRF. |
| `CORE-000267` | negative/01 | 6 | 12 | 0 | 6 | `94e9b76abddebba1` | `cardinality`; official omits rows where --PTCD is populated and --DECOD is empty. |
| `CORE-000268` | negative/02 | 12 | 22 | 0 | 10 | `22f53a2f22f19065` | `cardinality`; official omits MH not-unique relationship rows. |
| `CORE-000289` | negative/01 | 4 | 0 | 4 | 0 | `0d6a9b0e0135d205` | `candidate-empty`; official values do not appear in the fixture data. |
| `CORE-000356` | negative/01 | 28 | 0 | n/a | n/a | n/a | `candidate-skip`; official lists LB variables beyond literal Required/null value metadata semantics. |
| `CORE-000356` | positive/01 | 0 | 0 | n/a | n/a | n/a | `candidate-skip`; paired positive case stays excluded with the reviewed value metadata oracle gap. |
| `CORE-000370` | negative/01 | 9 | 8 | 9 | 8 | `8adae3b453bf4198` | `cardinality`; official reports DS death records for RFICDTC rule. |
| `CORE-000438` | negative/01 | 1 | 2 | 1 | 2 | `fc8d36a52ca37e62` | `cardinality`; official reports QVAL despite QNAM/QLABEL outcome variables. |
| `CORE-000438` | positive/01 | 1 | 0 | 1 | 0 | `d2743b58a0d519dc` | `positive-fixture`; positive fixture has populated QVAL but official reports an issue. |
| `CORE-000454` | negative/02 | 0 | 3 | 0 | 3 | `f5b1979c94a5cef4` | `official-empty`; DM end date differs from latest EX exposure date in fixture. |
| `CORE-000458` | negative/02 | 33 | 24 | 9 | 0 | `b52ad5cb802cd458` | `cardinality`; official includes POOLID values absent from fixture data. |
| `CORE-000529` | negative/01 | 28 | 24 | 4 | 0 | `fa23c3c92783ecc8` | `cardinality`; official includes DM row with empty DMDY despite --DY non_empty condition. |
| `CORE-000542` | negative/01 | 14 | 26 | 0 | 12 | `43bf8a16fcdb3c0e` | `cardinality`; official omits numeric --STRESC/--STRESN mismatch rows. |
| `CORE-000546` | positive/01 | 6 | 0 | 6 | 0 | `0eab22ce05f6b2ee` | `positive-fixture`; positive official result contains DS issues. |
| `CORE-000554` | negative/01 | 2 | 0 | 2 | 0 | `42068c79d6f43e5e` | `candidate-empty`; official reports RSTAGE populated although fixture row is blank. |
| `CORE-000569` | negative/01 | 3 | 2 | 1 | 0 | `4a87c66d205e89cd` | `cardinality`; official flags ARM1 though rule regex allows uppercase letters and digits. |
| `CORE-000570` | negative/01 | 2 | 0 | 2 | 0 | `22a0d031283aa53c` | `candidate-empty`; official reports empty USUBJID although fixture row has USUBJID. |
| `CORE-000648` | negative/01 | 33 | 2 | 31 | 0 | `e41b47f12dc93549` | `cardinality`; official AGE/AGETXT rows do not match fixture values. |
| `CORE-000648` | positive/01 | 0 | 2 | 0 | 2 | `d53b860e90366acf` | `positive-fixture`; positive fixture still produces candidate AGE/AGETXT issues. |
| `CORE-000652` | negative/01 | 2 | 0 | n/a | n/a | n/a | `candidate-skip`; external-distinct probe found STDY02-101 while official flags STDY02-99, which is present in DM/EX. |
| `CORE-000652` | positive/01 | 0 | 0 | n/a | n/a | n/a | `candidate-skip`; paired positive case stays excluded with the reviewed containment oracle gap. |
| `CORE-000698` | negative/01 | 36 | 24 | 12 | 0 | `2d5714b09294d070` | `cardinality`; official uses PDVALMAX rows for PDVALMIN rule. |
| `CORE-000698` | negative/02 | 12 | 20 | 12 | 20 | `2c236ef4c8de4acf` | `identity`; official uses PDVALMAX rows for PDVALMIN rule. |
| `CORE-000698` | positive/01 | 0 | 4 | 0 | 4 | `ecb12e8d0d2f66e4` | `positive-fixture`; PDVALMIN positive fixture has candidate issues. |
| `CORE-000704` | negative/01 | 36 | 24 | 12 | 0 | `4580e16d2c187249` | `cardinality`; official uses PDVALMIN rows for PDVALMAX rule. |
| `CORE-000704` | negative/02 | 20 | 12 | 20 | 12 | `c76bfbc834a1849b` | `identity`; official uses PDVALMIN rows for PDVALMAX rule. |
| `CORE-000704` | positive/01 | 0 | 4 | 0 | 4 | `1b77302593f00a70` | `positive-fixture`; PDVALMAX positive fixture has candidate issues. |
| `CORE-000718` | negative/01 | 0 | 8 | 0 | 8 | `f7d0ada74ab8c091` | `official-empty`; official/candidate disagree on --STDTC greater-than --ENDTC fixture values. |
| `CORE-000718` | positive/01 | 12 | 0 | 12 | 0 | `30738389cb726821` | `positive-fixture`; official/candidate disagree on --STDTC greater-than --ENDTC fixture values. |
| `CORE-000750` | negative/01 | 72 | 72 | 48 | 48 | `4725eee7bfa35d9c` | `identity`; official duplicates split LBDS rows under LBAE dataset. |
| `CORE-000770` | negative/01 | 15 | 12 | 3 | 0 | `1bba1023ab41c7f8` | `cardinality`; official includes TX Record 8 although fixture has seven TX rows. |
| `CORE-000814` | negative/01 | 24 | 48 | 0 | 24 | `d30e1f8b06b9e131` | `cardinality`; official omits StudyVersion_2 governance date rows present in fixture data. |
| `CORE-000865` | negative/01 | 4 | 2 | 2 | 0 | `69eed671581f6f08` | `cardinality`; official includes an empty PP issue row. |
| `CORE-000960` | negative/01 | 80 | 75 | 5 | 0 | `49226f3865943b03` | `cardinality`; XHTML flattening differs around malformed image data URI fixture. |

## Proposed Upstream Issue Bundles

Use these bundles to file small, reviewable upstream issues or PRs. Keep each
bundle focused on oracle/data reconciliation; do not implement core-rust
compatibility hacks to force these cases into `supported_match`.

| Bundle | Representative rules | Why bundled | Suggested upstream request |
|---|---|---|---|
| Positive fixtures with issues | `CORE-000014`, `CORE-000143`, `CORE-000172`, `CORE-000438`, `CORE-000546`, `CORE-000648`, `CORE-000698`, `CORE-000704`, `CORE-000718` | Positive fixtures should generally be clean examples, but these cases contain official or candidate issues. | Confirm whether the positive fixtures should be corrected, or whether the official `results.csv` should intentionally contain issues with explanatory metadata. |
| Official empty but candidate non-empty | `CORE-000080`, `CORE-000081`, `CORE-000108`, `CORE-000172`, `CORE-000252`, `CORE-000262`, `CORE-000454`, `CORE-000718` | Official `results.csv` is empty while the fixture/rule produces structural candidate issues. | Confirm rule applicability and update either fixture data or expected `results.csv`. |
| Official issue absent in candidate | `CORE-000224`, `CORE-000289`, `CORE-000554`, `CORE-000570` | Official rows refer to values or variables that candidate execution cannot find in the fixture/rule semantics. | Verify that official expected rows refer to actual fixture records and rule output variables. |
| Empty/non_empty and placeholder scope | `CORE-000117`, `CORE-000225`, `CORE-000262`, `CORE-000267`, `CORE-000438`, `CORE-000529`, `CORE-000554`, `CORE-000570`, `CORE-000648`, `CORE-000865` | Many discrepancies involve nullish values, placeholder variables, or condition/output-context variables. | Clarify expected nullish semantics and whether context variables should appear in expected output rows. |
| Domain placeholder column-ref comparators | `CORE-000195`, `CORE-000197`, `CORE-000198`, `CORE-000542` | Official cardinality differs around domain-placeholder variable references and comparator output. | Confirm whether official rows should include all fixture rows satisfying the placeholder-expanded condition. |
| Not-unique and relationship scope | `CORE-000184`, `CORE-000268`, `CORE-000750` | The rules involve relationship/group identity where official and candidate row identities or counts differ. | Clarify grouping keys, split-domain identity, and expected duplicate row emission. |
| PDVAL min/max paired rules | `CORE-000698`, `CORE-000704` | The paired rules appear to use the opposite PDVAL bound in official results and include positive fixture issues. | Review PDVALMIN/PDVALMAX fixture rows and expected results together to avoid fixing only one side of the pair. |
| Operation and join families | `CORE-000454`, `CORE-000770`, `CORE-000814`, `CORE-000960` | These discrepancies depend on operation-derived values, USDM joins, or XHTML flattening. | Confirm operation inputs, join cardinality, and flattened structural keys before treating candidate differences as engine bugs. |
| Candidate skipped pending oracle reconciliation | `CORE-000356`, `CORE-000652` | Candidate execution is intentionally skipped, and targeted review showed the official oracle does not match the literal rule semantics. | Reconcile official expected rows first; after upstream alignment, decide whether native semantics should be implemented. |

## Filing Order

1. File the paired or high-cardinality bundles first: PDVAL min/max,
   official-empty large-output (`CORE-000080`, `CORE-000081`), and positive
   fixtures with issues.
2. Then file focused semantic bundles: empty/non_empty, domain-placeholder
   comparators, and not-unique relationship scope.
3. Keep candidate-skipped bundles last. They should not become native
   implementation work until upstream confirms the expected rows.

Copy-pasteable upstream issue drafts for the first filing wave are maintained in
[`open-rules-upstream-issue-drafts.md`](open-rules-upstream-issue-drafts.md).

## Upstream Issue Template

For each proposed upstream issue or PR, include:

- Rule id and case id.
- The exact official `results.csv` row count.
- The candidate structural issue count from core-rust v29.
- A small excerpt of the input rows that demonstrate the discrepancy.
- Whether the discrepancy is cardinality, positive-fixture issue presence,
  official-empty/candidate-nonempty, or locator identity.
- The `issue_fingerprint_hash` from the v29 scoreboard, so future fixture or
  oracle changes can be compared without relying on diagnostic text.
- A note that the case remains excluded from supported accuracy until upstream
  oracle/data are reconciled.
