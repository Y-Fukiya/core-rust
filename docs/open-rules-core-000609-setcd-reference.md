# CORE-000609: TX.SETCD reference

CORE-000609 checks whether each `SETCD` occurs in the distinct values of
`TX.SETCD`. The operation selects the reference source, not the datasets to
validate. Previously Rust restricted evaluation to TX and included empty
character values in the distinct set. It reported no issues in all three
fixtures with official findings.

The rule now compares scoped rows against a set of exact, nonempty TX strings.
DM, RELREF, and TX share that set. Empty target values fail the check even
when TX contains no nonempty values. Without TX, the rule skips. The existing
CORE-000620 check still applies only to DM and is unchanged.

For generic CSV, this check uses the original SETCD text, including codes
that resemble numbers. Other rules use the same inferred types as before.
The adapter reads the actual SETCD column even when a similarly named column
such as `$SETCD` is present. A mixed run with CORE-000891 verifies that
CORE-000609 does not change how another rule groups these CSV rows.

## Official replay

The [CORE v1.0.0 Apple Silicon release](https://github.com/cdisc-org/cdisc-rules-engine/releases/tag/v1.0.0)
ran the local rule and fixture `.env` for candidate commit
`1881cb9a1f9a38b6cafc785eca5169b240981f66` and accepted commit
`7f7fae49376b3d023563ebb6c36a3b392d6e649f`. The official executable
and shipped cache hashes matched the previous v1.0.0 audit. The candidate
snapshot's stored CSVs matched the fresh official CSVs.

| Case | Official and corrected Rust findings | Record numbers |
| --- | ---: | --- |
| negative/01 | 14 | DM 1-3, RELREF 1-10, TX 41 |
| positive/01 | 2 | RELREF 7, TX 28 |
| positive/02 | 0 (skipped) | TX absent |
| positive/03 | 1 | TX 28 |

Both corpora matched the fresh official JSON on rule, dataset, record, output
variables, USUBJID, and sequence. The findings correspond to 28, 4, and 2
variable rows in the three stored candidate CSVs.

The default scorer matches all three cases against those CSVs. Strict CSV
comparison still reports missing/extra identities because the stored CSVs
omit USUBJID and SEQ, which the fresh official JSON and Rust preserve.
The absent-TX case stays `skipped_unsupported` in the candidate scoreboard;
the official runtime also skips, but its stored CSV contains no issues.
The accepted snapshot has no stored results CSV for any of the four cases,
so its scoreboard continues to classify them as `no_official_oracle`.

The full candidate run scored 2,298 cases. Supported mismatches fell from
33 to 30, with 17 deferred mismatches unchanged. The full accepted run
scored 2,296 cases with no new mismatch. Only CORE-000609 case records changed
in either comparison. The pins, stored oracles, baselines, and scorer are
unchanged.

Local replay inputs and reports are under `target/core-609-setcd-audit/`
and `target/core-609-accepted-audit/`. The final candidate and accepted
scoreboards are in the first directory's `final-reviewed/` subdirectory.
