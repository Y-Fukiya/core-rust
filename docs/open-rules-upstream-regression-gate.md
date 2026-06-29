# Full Upstream Regression Gate Plan

The scheduled full upstream workflow is intentionally an observe job. It should
stay green when compatibility gaps exist, as long as the scorer produces a
usable `scoreboard.json` artifact.

The next maturity step is a separate full upstream regression gate. This gate
should compare a current full-corpus scoreboard against an accepted full
upstream baseline and fail only when behavior regresses.

## Proposed Workflow Shape

Keep three layers separate:

| Layer | Trigger | Purpose | Failure policy |
| --- | --- | --- | --- |
| Repository-local fixture gate | Pull request | Fast enforced regression gate | fail on mismatch, harness error, coverage below 100%, or skipped unsupported |
| Full upstream observe | Scheduled / manual | Produce current full-corpus artifacts | fail only when `scoreboard.json` is not generated |
| Full upstream regression gate | Manual / release candidate | Compare against accepted full upstream baseline | fail on regression from baseline |

Do not make the full upstream regression gate a required PR check until the
runtime and artifact size are predictable enough for normal review cadence.

## Baseline Candidate

Use the existing `xtask open-rules baseline` command with a separate full
upstream baseline file, for example:

```sh
cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard-upstream/scoreboard.json \
  --baseline tests/open_rules/upstream-baseline.json
```

The baseline file should be generated from a reviewed scheduled run, committed
in a dedicated PR, and tied to `tests/open_rules/upstream.lock`.

## Regression Policy

The full upstream baseline should fail when:

- a baseline `supported_match` case becomes any non-match bucket
- a new `supported_mismatch` case appears
- a new `harness_error` case appears
- a new `mixed_skipped_and_issues` case appears
- `coverage` decreases
- `skipped_unsupported` increases
- a baseline case disappears from the current scoreboard

Improvements should be allowed and reported:

- `skipped_unsupported` becoming `supported_match`
- `no_official_oracle` becoming official-oracle-backed `supported_match`
- `rule_id_hand_port` cases becoming `native_engine` cases while staying
  `supported_match`

## Adoption Checklist

1. Run the scheduled observe workflow and download `scoreboard.json`.
2. Review `supported_mismatch`, `harness_error`, `mixed_skipped_and_issues`,
   `no_official_oracle`, and skipped reasons.
3. Commit the reviewed full upstream baseline as
   `tests/open_rules/upstream-baseline.json`.
4. Add a manual workflow that re-runs full upstream scoring and calls
   `xtask open-rules baseline` against that baseline.
5. Keep the scheduled observe workflow non-blocking for compatibility gaps.
6. Promote the manual regression workflow to release-candidate gating only after
   several stable runs.

## Non-Goals

- Do not require 100% full upstream coverage.
- Do not fail scheduled observe runs for known compatibility gaps.
- Do not treat historical output directories as current conformance claims.
- Do not combine baseline updates with rule semantics changes unless the diff is
  intentionally reviewed as a compatibility change.
