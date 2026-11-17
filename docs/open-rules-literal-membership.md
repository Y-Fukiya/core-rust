# Literal membership lists and CORE-000123

CORE-000123 through CORE-000131 falsely reported the allowed values `Y` and
`N` when a rule declared `value_is_literal: true`. The correction is in the
engine's containment evaluation. It does not change the scorer, compatibility
manifests, committed baselines, or accepted Open Rules pin.

## Cause and correction

The candidate rules at `1881cb9a1f9a38b6cafc785eca5169b240981f66` combine
`non_empty` with `is_not_contained_by`, the array `[Y, N]`, and
`value_is_literal: true`. YAML loading already preserves `Y` and `N` as strings.

`value_expr_from_condition_value` preserves explicit literals in
`ValueExpr::Literal`, including arrays. The scalar containment evaluator
iterates `ValueExpr::List`, but converts a literal array into one JSON string.
It therefore compared each `Y` or `N` against the single value `["Y","N"]`.
Both valid rows failed membership.

The stored expression remains `Literal(Array)`. Only `is_contained_by`,
`is_not_contained_by`, and their case-insensitive variants create a temporary
`ValueExpr::List` for evaluation. This copy is made once before evaluating
rows. Elements remain literal values, including strings that resemble column
references. Other operators and the shared scalar matcher remain unchanged.

An initial parser-side correction was rejected during review. Turning a
literal array into a stored `List` exposes its strings to join reference
discovery. A native CLI probe with one DM join, target `X.DM`, and literal
`["DM.M"]` reproduced a false pass because the inferred join prefix changed.
Keeping the literal expression intact prevents this regression without
changing join semantics or adding a new model variant.

The API regression first reported two errors instead of zero for the valid
Y/N dataset. Regression coverage also preserves the literal model through
serialization and checks joined-dataset behavior.

## Official v1.0 verification

On 2026-11-17, the official v1.0.0 Apple Silicon executable ran all 18 original
positive and negative fixtures for CORE-000123 through CORE-000131. The fixture
environments, rules, data, and bundled cache were preserved. Each positive run
returned `SUCCESS` with zero CSV rows. Each negative run returned
`ISSUE REPORTED` with one CSV row. All 18 fresh CSVs equal the stored oracle
after CSV parsing.

Runtime provenance:

- Official engine source `f3282a0d870bbdf57e326e9992cceea3ed0d9af5`.
- Executable SHA-256 `fd0fc1b20122b6adbbab0ba6b0e6c4c959415943b9982d0f4c6571b0fcfea16c`.
- Cache tree SHA-256 `9b0d65304dced1746f32d5b3f5a7225519925746a9405db3ef3fd10a6b748c8d`.
- Isolated environment with `CACHE_TYPE=in_memory`, one process, and each
  fixture's `.env` standard and version.

This macOS diagnostic is not a replacement for the Linux-pinned adoption audit.
Local evidence is in `target/core-123-literal-list-audit/`, including the probe,
input hashes, process arguments, logs, and fresh reports. These ignored local
artifacts need archiving before `target` cleanup.

## Family result and remaining identity differences

| Cases | Official rows per case | Candidate before | Candidate after |
| --- | ---: | ---: | ---: |
| CORE-000123 through CORE-000131 positive/01 | 0 | 2 | 0 |
| CORE-000123 through CORE-000131 negative/01 | 1 | 1 | 1 |

Default scoring improves from eight matches and ten mismatches to 17 matches
and one mismatch. CORE-000124 negative/01 still reports candidate Record `2`
against official Record `1`. Its physical CSV has one row with `AESEQ=2` and
`AESCONG=U`. This existing locator difference is not a membership failure and
is not hidden by a new normalization or exclusion.

Strict scoring matches all nine positive cases. All nine negative cases still
have existing identity differences because the candidate includes USUBJID and
sequence values absent from the official CSV. CORE-000124 also has the Record
difference above. Matching detection counts does not imply strict identity
conformance.

## Candidate corpus impact

The final implementation executes all 2,298 cases in the candidate snapshot
without harness errors. Compared with the preceding Domain Presence change,
21 cases change and 20 become matches under the unchanged default scorer.
No previously supported match is lost.

| Default bucket | Before | After |
| --- | ---: | ---: |
| Supported match | 2,109 | 2,129 |
| Supported mismatch | 53 | 37 |
| Deferred mismatch | 21 | 17 |
| Deferred skipped | 48 | 48 |
| Unsupported skipped | 55 | 55 |
| No official oracle | 12 | 12 |
| Harness error | 0 | 0 |

The other affected rules are CORE-000087, 324, 376, 572, 710, and 763. They
also use explicit literal arrays with `is_not_contained_by`. CORE-000376
negative/02 improves from 48 to 20 candidate rows against six official rows,
but remains a mismatch. The other 20 changed cases become default matches.

Strict scoring improves from 1,275 matches and 922 mismatches to 1,291 matches
and 906 mismatches. Its 89 unsupported skips and 12 missing-oracle cases remain
unchanged. The default adoption gate still fails with 54 mismatching cases.
This result does not justify updating the accepted pin or baseline.

The accepted snapshot `7f7fae49376b3d023563ebb6c36a3b392d6e649f` also executes
all 2,296 cases without harness errors. All case buckets, counts, mismatch
fingerprints, provenance classifications, and normalization traces are
unchanged. Its existing 17 supported and seven deferred mismatches remain.
The accepted full regression gate is not green, but this correction adds no
regression.

One earlier accepted-corpus attempt overlapped test linking and hit two
`No space left on device` report-write failures. It is not counted as a
successful run. The final run above was repeated after compilation completed.
The evidence directory contains final candidate and accepted scoreboards,
run summaries, the strict candidate scoreboard, and `impact.json` with the
case-level comparison. The original fixtures and stored oracles are unchanged.

## Regression protection

`literal_membership_tests.rs` tests explicit arrays, bare YAML `Y/N`, empty
lists, scalar and object literals, and unchanged non-membership operators.
`open_rules_literal_membership.rs` exercises all nine AE flag rules with both
compatibility settings. It checks valid `Y/N/empty`, invalid values, execution
status, variables, row, USUBJID, and sequence. Additional execution cases test
all four containment operators and literal names despite a colliding column.
Direct-engine and serialized native-rule checks include nulls, numbers, and
empty arrays. They also assert that equality, prefix, suffix, and `contains_all`
keep their previous literal-array behavior. The single-DM join regression
checks the issue's full structural identity with both compatibility settings.

```sh
cargo test -p core-rule-model --locked --lib literal_membership_tests
cargo test -p core-api --locked --lib open_rules_literal_membership
```

The final local verification passed all four workspace commands below, with
814 passing tests and no failed tests. The focused modules contribute four
parser tests and five API tests. A rebuilt native CLI also reports one finding
and exits nonzero under `--strict` for the joined invalid-value probe.

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
```

The final independent source review found no actionable correctness findings.
The reviewer did not execute these runtime checks. Logs are stored as
`final-build.log`, `final-check.log`, `final-clippy.log`, and `final-test.log`
under the local evidence directory. These are local checks, not a new GitHub
Actions run. This change does not authorize a merge or an adopted-rules update.
