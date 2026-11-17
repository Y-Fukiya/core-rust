# CORE-000206: parent record references

CORE-000206 checks that a populated `IDVARVAL` matches the `IDVAR` column
in the parent domain named by `RDOMAIN`. A match for another subject does
not satisfy a subject-specific reference.

Two independent problems affected the candidate cases:

- The SUPP path skipped validation when no parent row held the referenced
  value. It therefore missed SUPPLB Record 1 in negative/02 (`LBSEQ=320`)
  and Record 2 in negative/03 (`LBSEQ=999`).
- The adapter hard-coded `USUBJID` in the output variable list. The candidate
  YAML declares only `RDOMAIN`, `IDVAR`, and `IDVARVAL`. The accepted YAML
  declares all four. The adapter uses the declared list and retains
  `USUBJID` in the record locator in both snapshots.

## Official replay

The [CORE v1.0.0 Apple Silicon release](https://github.com/cdisc-org/cdisc-rules-engine/releases/tag/v1.0.0)
ran the local rule, the shipped cache, and each fixture's own `.env`.
The engine and cache hashes matched the prior official audit. The candidate
checkout was `1881cb9a1f9a38b6cafc785eca5169b240981f66`.

| Candidate case | Original Rust CSV rows | Corrected Rust CSV rows | Stored CSV rows | Fresh official CSV rows |
| --- | ---: | ---: | ---: | ---: |
| negative/01 | 16 | 12 | 12 | 12 |
| negative/02 | 16 | 15 | 15 | 15 |
| negative/03 | 8 | 9 | 9 | 84 |

The first two fresh outputs matched the stored CSVs. In negative/03, multiple
child records share one subject. Joining by `USUBJID` alone produces repeated
findings and flags records with valid references. On an isolated copy,
adding `IDVAR` and `IDVARVAL` to the three Match Datasets key lists yielded
only CO Record 2, RELREC Record 2, and SUPPLB Record 2. These are the three
physical records in the stored oracle. The control changed the rule's join
keys only. The Rust fix does not replicate duplicate reports or adopt that
modified rule.

Separate official negative/02 controls with one SUPPLB row passed with a
matching parent value and reported the row if the referenced column or
parent dataset was absent. All three original candidate positive cases
passed the official replay.

The corrected run matched dataset, record number, output variable set,
and subject for negative/01 and negative/02 against the replayed JSON.
Negative/03 matched the compound-key control on the same fields. The three
positive cases still passed. In the full candidate run, supported
mismatches fell from 30 to 27 and the 17 deferred mismatches were unchanged.
No other rule's case records changed.

## Accepted snapshot

The accepted snapshot remains `7f7fae49376b3d023563ebb6c36a3b392d6e649f`.
Its negative/02 stored CSV has 16 variable rows. A fresh official replay
has 20. The four additional rows are the four declared output variables
for the same previously missed SUPPLB record. The stored CSV therefore
needs separate oracle review before this change can pass the pinned
curated gate. No committed fixture, baseline, scorer, or upstream pin
is changed here.

The full accepted run changed only this case. The pinned curated subset
matched 277 of 278 cases. A temporary copy using the fresh official CSV for
this case matched all 278 and passed the existing baseline comparison with
zero regressions or review-required entries.

The local evidence lives under `target/core-206-idvarval-audit/`:
`official/`, `controls-v2/`, and `boundary/` include the input copies,
process logs, and official JSON and CSV reports. The final scoreboards are
under `after/`.
