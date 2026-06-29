# Skipped Coverage Gaps: Next Actions (SD1234 / SD1326)

## Current skipped candidate set

The final SDTM-IG pilot run leaves `5` rules in coverage-gap state:

- `P21PORT-SDTMIG-SD1234-3E0AC03D`
- `P21PORT-SDTMIG-SD1234-9F87FE70`
- `P21PORT-SDTMIG-SD1234-DF6DFC87`
- `P21PORT-SDTMIG-SD1326-83FF9D4A`
- `P21PORT-SDTMIG-SD1326-C8AE7D49`

These are `ACTUAL_SKIPPED_BY_CORE` in the official CORE compare output
and are currently counted as coverage gaps, not supported mismatches.

## Why this is low risk to merge now

- No `supported mismatch` cases were observed in the final run.
- All non-pass rows are from `ACTUAL_SKIPPED_BY_CORE` and are explicitly
  separated in failure classification.
- Keeping them out of PR export prevents potentially unresolved semantic cases
  from being treated as accepted conversion output.

## Proposed follow-up tasks

1. Keep these 5 IDs in a dedicated skipped bucket (`SKIPPED_COVERAGE_GAP`).
2. Investigate rule-level behavior for `SD1234` and `SD1326` with official CORE
   input permutations, and attach reproduction logs in one follow-up issue.
3. Re-run a targeted `run-core` + `compare-results` for only these 5 rules
   before changing default export policy.
4. On successful reproduction, promote status from coverage gap to:
   - `AUTO_CONVERTIBLE` if deterministic PASS generation is proven.
   - `SKELETON_ONLY` if semantic implementation remains unresolved.

## Gate for next phase

- Do not widen export scope until these IDs pass official comparison or are
  intentionally marked as known gaps.

