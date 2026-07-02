# Open Rules Positive-Zero Probe Inventory

This note tracks the positive-zero probe rule ids that remain in the
oracle-gap manifest. They are not active full-upstream scoreboard failures in
the accepted baseline; they are promotion guards that prevent rule families
from being moved into supported scoring without family-level evidence.

## Current State

As of the current upstream baseline:

| Item | Count |
|---|---:|
| `defer_positive_zero_probe` manifest entries | 141 |
| `unsafe_positive_zero_probe` manifest entries | 2 |
| Active positive-zero cases in `tests/open_rules/upstream-baseline.json` | 0 |

The accepted scoreboard therefore has no positive-zero `supported_mismatch`,
`deferred_oracle_gap_mismatch`, or `deferred_oracle_gap_skipped` cases. The
manifest entries are retained as a reviewed no-auto-promotion guard.

## Review Rule

Do not remove a positive-zero probe entry only to improve aggregate coverage.
Remove or recategorize entries only after a rule-family review shows that:

- the candidate engine output is computed from input data, not oracle output;
- strict scoring does not introduce a supported mismatch for the family;
- the upstream official oracle and fixture are suitable correctness evidence;
- the updated default and strict scoreboards are regenerated and compared.

If a family cannot satisfy those conditions, leave it in the manifest and
treat it as known-unknown compatibility debt rather than native engine
conformance evidence.
