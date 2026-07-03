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
| Positive-zero-probe-classified active scoreboard failures | 0 |
| Baseline cases whose rule id is in a positive-zero probe manifest | 355 |
| Positive-zero manifest cases scored as `supported_match` | 316 |
| Positive-zero manifest cases deferred for other reviewed reasons | 31 |
| Positive-zero manifest cases with `no_official_oracle` | 8 |

The accepted scoreboard has no cases whose active bucket or deferral reason is
attributed to the positive-zero probe itself. Some positive-zero manifest rule
ids are still deferred for other reviewed reasons. The manifest entries are
retained as a reviewed no-auto-promotion guard, and the positive-zero rule ids
remain present in the upstream baseline.

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
