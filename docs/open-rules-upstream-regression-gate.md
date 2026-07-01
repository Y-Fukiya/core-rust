# Open Rules Upstream Regression Gate

The weekly upstream workflow has two lanes:

- `upstream-observe` always runs the pinned full corpus, uploads artifacts, and
  only fails when the scoreboard is not generated.
- `upstream-regression` runs only when
  `tests/open_rules/upstream-baseline.json` exists. It compares the generated
  full-corpus scoreboard against that accepted baseline.

The full upstream baseline should fail when:

- a baseline `supported_match` case becomes any other bucket
- a native-engine `supported_match` case moves to `rule_id_hand_port` or
  `unknown` provenance
- a `supported_mismatch` case is newly deferred as
  `deferred_oracle_gap_mismatch` without reviewed baseline acceptance
- a new `supported_mismatch` case appears
- a new `harness_error` case appears
- a new `mixed_skipped_and_issues` case appears
- `deferred_oracle_gap_mismatch` increases
- `coverage` decreases
- `skipped_unsupported` increases
- same-bucket issue counts or issue fingerprints regress
- a baseline case disappears from the current scoreboard

The full upstream baseline should warn, but not fail, when:

- `deferred_oracle_gap_skipped` increases

The committed upstream baseline strips per-case `missing` and `extra` issue
arrays so it remains portable and reviewable, but keeps `missing_count`,
`extra_count`, and `issue_fingerprint_hash`. The comparator uses those portable
fields before falling back to arrays, so stripped baselines can still detect
same-bucket issue regressions without creating false positives.

Do not use a hard 100% full-corpus gate. The accepted upstream baseline is a
"do not get worse" guard, not a conformance certificate. The full upstream
workflow currently runs on `workflow_dispatch` and a weekly schedule; normal PR
CI still uses the repository-local fixture gate unless a curated upstream subset
is added to CI.
