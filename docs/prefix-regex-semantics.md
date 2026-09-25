# Positive Prefix Regex Support

This change implements the generic `prefix_matches_regex` operator used by
CORE-000107 in proposed upstream revision
`1fb7b81e40bdb6632375761c561fabd29676a477`. It does not adopt that revision,
change accepted baselines, reclassify gaps, or add rule-ID-specific execution.

## Semantic Basis

The [official engine implementation](https://github.com/cdisc-org/cdisc-rules-engine/blob/3b330f06e3fb881dcba1e1965e9ad4543a2226a4/cdisc_rules_engine/check_operators/dataframe_operators.py)
uses a non-null guard and regex search on the first `prefix` characters, not a
full-string match. Null never matches; an empty string can match `^$`. A zero
length prefix is an empty string for any non-null input. Unicode slicing is by
characters, not UTF-8 bytes. Numeric values use the engine's existing scalar
string conversion. Regex syntax remains the Rust regex crate's supported subset,
not arbitrary Python regex syntax.

Tests cover search versus full-match behavior, excluded suffixes, Unicode,
empty/null values, zero and oversized prefixes, numeric values, invalid
configuration, and API preflight/execution of the APID missing-column branch.
The existing negative operator's suppression of empty values is intentionally
unchanged; this change does not claim the two operators are complements on
empty input. That older semantic difference requires separate review.

## Evidence Boundary

Previously the model classified the positive operator as unsupported; the new
regression test reproduces that failure before implementation. Supporting it
removes an execution blocker, not proof that the proposed CORE-000107 fixtures
now match. The September 25 local run executed all nine official cases with
strict scoring: four positive matches, five negative mismatches, zero skips,
and zero harness errors. No score normalization or gap reclassification was
used.

| Case | Official issue rows | Candidate issue rows |
|---|---:|---:|
| negative/01 | 9 | 40068 |
| negative/02 | 9 | 18 |
| negative/03 | 9 | 27 |
| negative/04 | 9 | 40068 |
| negative/05 | 45 | 135 |
| positive/01 through positive/04 | 0 each | 0 each |

The negative oracle has dataset-level findings with blank Record locators;
candidate findings repeat on data rows. Review dataset-sensitive presence
conditions and output context next. Do not erase candidate findings in the
scorer, call these matches, or promote the proposed pin on this evidence.
Other proposed-pin mismatches and unsupported operators remain separate work.

Reproduce using an official archive/check-out at the proposed SHA, with only
`Published/CORE-000107` copied into a fresh subset directory:

```sh
cargo run --locked -p xtask -- open-rules run-score \
  --open-rules-root /path/to/proposed-prefix-subset \
  --core-rs-results-root /path/to/new-audit/candidate \
  --out /path/to/new-audit/strict --strict-scoring
```

The expected exit is nonzero while the five mismatches remain. Keep the
scoreboard and candidate reports. A subset archive has no Git HEAD, so verify
the archive revision separately; do not claim `--strict-lock` verification for
it. The accepted lock/baselines remain unchanged.
