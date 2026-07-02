# Open Rules Upstream Run

This note records the accepted pinned upstream baseline for the Open Rules
oracle harness. Earlier Phase 6 numbers in this file were intentionally removed
because they predated the strict-scoring audit mode, portable baselines, and
scoring-normalization accounting.

The committed source of truth is
`tests/open_rules/upstream-baseline.json`, generated from the pinned
`cdisc-org/cdisc-open-rules` SHA in `tests/open_rules/upstream.lock`.

## Current Accepted Baseline

| Metric | Value |
|---|---:|
| Total cases | 2296 |
| Supported match | 2059 |
| Supported mismatch | 0 |
| Skipped unsupported | 0 |
| Deferred oracle-gap mismatch | 0 |
| Deferred oracle-gap skipped | 55 |
| No official oracle | 182 |
| Harness error | 0 |
| Coverage | 89.68% |

`supported_accuracy = 100%` is not a full-corpus conformance claim. It only
means that the currently supported denominator has no supported mismatches under
the default compatibility scoring policy. Read it with the denominator split,
execution provenance, scoring policy, and strict-scoring audit output.

## Evidence Split

The default compatibility scoreboard splits supported matches by scoring
policy:

| Scoring policy | Supported match | Coverage |
|---|---:|---:|
| Strict identity | 2010 | 87.54% |
| Oracle-gap normalized | 49 | 2.13% |

The remaining scorer-side normalizations are:

| Normalization | Cases |
|---|---:|
| `output_context_variable_aligned` | 10 |
| `row_locator_identity_relaxed` | 47 |

Strict scoring disables oracle-gap reclassification and oracle-informed
identity/output-context normalization. It is expected to be harsher and should
be reported beside the default compatibility scoreboard as an audit lens, not
used as the default regression gate.

## Deferred Cases

The accepted deferred-skipped cases are not supported matches and are not
conformance evidence:

| Category | Cases |
|---|---:|
| `official_oracle_fixture_gap` | 51 |
| `standard_filter_oracle_gap` | 4 |

These cases should not grow silently. The baseline comparator treats increases
in `deferred_oracle_gap_skipped`, `deferred_oracle_gap_mismatch`,
`skipped_unsupported`, `harness_error`, supported mismatches, and scorer
normalization usage as regression or review-required signals.

## Gate Model

Normal PR CI runs a repository-local fixture gate plus curated upstream
supported and gap subsets. The full pinned upstream corpus runs as a scheduled
or manually triggered workflow. That means a regression outside the curated PR
subsets can remain green on `main` until the next full upstream run. Use the
scheduled/manual artifacts to review the default compatibility scoreboard and
the strict-scoring audit scoreboard together.
