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
- a new `supported_mismatch` case appears
- a new `harness_error` case appears
- a new `mixed_skipped_and_issues` case appears
- `coverage` decreases
- `skipped_unsupported` increases
- a baseline case disappears from the current scoreboard

Do not use a hard 100% full-corpus gate until the accepted baseline is reviewed
and checked in. Until then, full upstream runs are observability artifacts, while
the repository-local fixture remains the PR correctness gate.
