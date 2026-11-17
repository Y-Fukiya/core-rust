# CORE-000677 POOLDEF presence

CORE-000677 and CORE-000896 ask different questions:

- CORE-000677 is a Dataset-sensitive Domain Presence Check. When a `POOLID`
  column exists but the `POOLDEF` dataset does not, report one `STUDY` issue
  for `$poolid_exists` and `POOLDEF`, without a record locator.
- CORE-000896 is a Record Data check. It checks nonempty `POOLID` values
  against the values in `POOLDEF` and reports the individual invalid records.

Both IDs previously entered the same membership override before rule-type
dispatch. That override passes when `POOLDEF` is absent. When `POOLDEF` is
present but omits some values, it reports individual record issues. Both
behaviors are wrong for the presence check. Restrict the override to
CORE-000896 and let CORE-000677 use the existing Domain Presence evaluator.
The evaluator uses one metadata row when its physical anchor is empty. A
study-level existence check must not depend on whether the anchor has any
data records.

## Official v1.0.0 comparison

The diagnostic re-ran the original cases against the official Apple Silicon
CORE v1.0.0 binary and bundled cache documented in the
[Record-number audit](open-rules-literal-membership.md#official-v10-verification).
It used one process and the unmodified fixture `.env` files. The candidate
rules snapshot is `1881cb9a1f9a38b6cafc785eca5169b240981f66`.

| CORE-000677 candidate case | Fresh official CSV rows | core-rust before | core-rust after |
| --- | ---: | ---: | ---: |
| negative/01 | 2 | 0 | 2 |
| negative/02 | 2 | 0 | 2 |
| positive/01 | 0 | 0 | 0 |
| positive/02 | 0 | 12 | 0 |
| positive/03 | 0 | 0 | 0 |

The counts above are variable rows after expansion, not distinct findings.
Each official negative case has one locator-free `STUDY` issue. All five
official CSV outputs equal the stored candidate oracles. A separate copy with
only the rule identifier changed to bypass the override also matched all five.
After the correction the original five cases match in both default and strict
scoring without a scorer change. The recorded execution provenance changes
from rule-ID hand-port to generic engine for this rule.

Fresh CORE-000896 negative/01 reports `VS`, Record `7`, `POOLID=POOL99`. Its
stored oracle includes an extra `$pooldef_poolid` context row. The correction
preserves the existing CORE-000896 record and output variables. The fresh
official JSON also reports numeric `SEQ=8`; the existing Rust CSV leaves
`seq` empty for that fixture's numeric `VSSEQ`. Both output differences are
outside this change. Positive/01 reports no issue.

Two additional controls use copies of candidate negative/01. The first adds
a header-only DM before VS. The second removes all VS records but retains
the `POOLID` header. Official v1.0.0 still reports one `STUDY` issue in each
control. Before the empty-anchor correction, the generic Rust evaluator
reported no issue for either.

## Regression verification

The tests exercise absence of `POOLDEF` with and without the `POOLID` column,
including a present column with a blank value. A shared fixture with `POOLDEF`
present and an unmatched ID passes CORE-000677 but reports the invalid row
under CORE-000896 in the default execution mode. The CORE-000896 regression
checks the dataset, variable names, row, subject, and sequence. The two tests
failed before separating the rules and now pass. Two more regressions cover
the header-only controls. Both also match the fresh official issue identities
in the real fixture runner.

```sh
cargo test -p core-api --locked --lib run_validation_pooldef
```

Workspace fmt, check, Clippy, and tests passed locally. Full
candidate execution completed 2,298 cases without harness errors. Default
supported mismatches dropped from 36 to 33, and deferred mismatches stayed at
17. All changes were confined to CORE-000677; CORE-000896 and the other rules
retain their previous comparison results.

## Accepted snapshot boundary

The accepted pin remains `7f7fae49376b3d023563ebb6c36a3b392d6e649f`. Its saved
CORE-000677 oracle expects 12 variable rows for each of negative/01 and
negative/02, and none for positive/02. Running those same inputs on official
v1.0.0 produces zero, zero, and two rows respectively. The old negative
fixtures contain `POOLDEF`; the old positive/02 has `POOLID` but no `POOLDEF`.
The labels and saved outputs are not reliable expectations for this rule.

The current curated upstream CI gate still uses the accepted snapshot and
requires a match against its saved CSVs. It needs a separate, reviewed
expectation update or the adoption of corrected upstream oracles. This change
does not rewrite the fixtures, change the scorer, or suppress these cases.

Full accepted execution completed 2,296 cases without harness errors. Its
default supported mismatches against stored oracles increased from 18 to 21.
In a local run of the CI subset, 275 of 278 cases matched and the only three
mismatches were the CORE-000677 cases above. The `run-score` gate exits 1.
This is not a new GitHub Actions run and the PR is not ready for adoption.

Local evidence is in `target/core-677-pooldef-audit/`, including the probe,
official JSON and CSV reports, per-case input hashes, full scoreboards under
`final/`, and `impact.json`. This macOS diagnostic is not a new Linux-pinned
adoption audit.
