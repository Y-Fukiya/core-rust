# Open Rules Upstream Issue Drafts

Snapshot: `target/open-rules-scoreboard-upstream-v29/scoreboard.json`

These are draft upstream issue bodies for the remaining reviewed
`deferred_oracle_gap_skipped` backlog. They are intentionally framed as
oracle/data reconciliation requests, not as core-rust conformance claims.

## Draft 1: PDVALMIN/PDVALMAX Paired Fixture Oracle Review

Title:

```text
Review CORE-000698/CORE-000704 expected results for paired PDVALMIN/PDVALMAX rules
```

Body:

```markdown
## Summary

The Open Rules fixtures for `CORE-000698` and `CORE-000704` appear to have
paired expected-result inconsistencies. In the v29 core-rust structural
comparison, these cases are excluded from supported accuracy as
`official_oracle_fixture_gap`.

This is not proposed as an engine compatibility workaround. The request is to
review whether the committed `results/results.csv` files match the rule text and
fixture data for the paired PDVAL minimum/maximum rules.

## Observed cases

| Rule | Case | Official issues | Candidate issues | Missing | Extra | Fingerprint |
|---|---|---:|---:|---:|---:|---|
| CORE-000698 | negative/01 | 36 | 24 | 12 | 0 | `2d5714b09294d070` |
| CORE-000698 | negative/02 | 12 | 20 | 12 | 20 | `2c236ef4c8de4acf` |
| CORE-000698 | positive/01 | 0 | 4 | 0 | 4 | `ecb12e8d0d2f66e4` |
| CORE-000704 | negative/01 | 36 | 24 | 12 | 0 | `4580e16d2c187249` |
| CORE-000704 | negative/02 | 20 | 12 | 20 | 12 | `c76bfbc834a1849b` |
| CORE-000704 | positive/01 | 0 | 4 | 0 | 4 | `1b77302593f00a70` |

## Review request

- Confirm whether the expected results for `CORE-000698` should reference
  PDVALMIN or PDVALMAX fixture rows.
- Confirm whether the expected results for `CORE-000704` should reference
  PDVALMAX or PDVALMIN fixture rows.
- Confirm whether the positive fixtures should intentionally produce issues.

Until reconciled, these cases are best treated as oracle/fixture review backlog
rather than native engine conformance evidence.
```

## Draft 2: Official Empty Results With Large Candidate Output

Title:

```text
Review empty expected results for CORE-000080 and CORE-000081 timing placeholder fixtures
```

Body:

```markdown
## Summary

Two Open Rules cases have empty official `results/results.csv` files while the
fixture/rule combination produces large structural candidate output in the v29
core-rust structural comparison.

These are currently excluded from supported accuracy as
`official_oracle_fixture_gap`. The request is to confirm whether the official
oracle is intentionally empty or whether fixture data / expected results should
be reconciled.

## Observed cases

| Rule | Case | Official issues | Candidate issues | Missing | Extra | Fingerprint |
|---|---|---:|---:|---:|---:|---|
| CORE-000080 | negative/01 | 0 | 6360 | 0 | 6360 | `c9cc4e7f39f2fa67` |
| CORE-000081 | negative/01 | 0 | 3232 | 0 | 3232 | `7d8f17c321ea1553` |

## Review request

- For `CORE-000080`, confirm whether the fixture rows containing `--TPTREF`
  without corresponding timing reference columns should produce expected issues.
- For `CORE-000081`, confirm whether the fixture rows containing `--STAT`
  without `--PRESP` should produce expected issues.
- If the official empty `results.csv` files are intentional, please add
  explanatory metadata or comments so downstream oracle harnesses can treat
  them as reviewed exceptions.

Until reconciled, these cases are best kept out of supported accuracy to avoid
turning an empty expected file into a false engine pass.
```

## Draft 3: Positive Fixture Cases With Official Or Candidate Issues

Title:

```text
Review positive Open Rules fixtures that contain expected or candidate issues
```

Body:

```markdown
## Summary

Several positive Open Rules fixtures contain official issues or produce
structural candidate issues in the v29 core-rust structural comparison. Because
positive fixtures are usually interpreted as clean examples, these cases are
currently excluded from supported accuracy as `official_oracle_fixture_gap`.

The request is to confirm whether these positive fixtures should be corrected,
or whether they intentionally contain issues and should carry explanatory
metadata.

## Observed cases

| Rule | Case | Official issues | Candidate issues | Missing | Extra | Fingerprint |
|---|---|---:|---:|---:|---:|---|
| CORE-000014 | positive/02 | 3 | 0 | 3 | 0 | `53fba3b163dbc15c` |
| CORE-000143 | positive/02 | 0 | 1 | 0 | 1 | `c205443c8aac7669` |
| CORE-000172 | positive/05 | 1 | 0 | 1 | 0 | `1dacf7ea9cc71476` |
| CORE-000438 | positive/01 | 1 | 0 | 1 | 0 | `d2743b58a0d519dc` |
| CORE-000546 | positive/01 | 6 | 0 | 6 | 0 | `0eab22ce05f6b2ee` |
| CORE-000648 | positive/01 | 0 | 2 | 0 | 2 | `d53b860e90366acf` |
| CORE-000698 | positive/01 | 0 | 4 | 0 | 4 | `ecb12e8d0d2f66e4` |
| CORE-000704 | positive/01 | 0 | 4 | 0 | 4 | `1b77302593f00a70` |
| CORE-000718 | positive/01 | 12 | 0 | 12 | 0 | `30738389cb726821` |

## Review request

- Confirm whether these positive fixtures should contain any expected issues.
- If yes, clarify how downstream oracle harnesses should interpret
  positive-fixture issue rows.
- If no, update either the fixture data or the expected `results.csv` files.

Until reconciled, these cases are best treated as oracle/fixture review backlog,
not as engine mismatches or supported matches.
```

## Draft 4: Standard Applicability Fixture Review

Title:

```text
Review standard applicability metadata for CORE-000217 and CORE-000478 fixtures
```

Body:

```markdown
## Summary

Four Open Rules cases are skipped by core-rust because the fixture
standard/version does not match the rule authority or applicability note. These
are classified as `standard_filter_oracle_gap`, not generic unsupported
coverage.

## Observed cases

| Rule | Cases | Candidate skip | Review finding |
|---|---|---|---|
| CORE-000217 | negative/05, positive/05 | `Requested rule CORE-000217 does not match standard filter sendig 3.1` | The rule authority covers SDTMIG/TIG, while the `/05` fixtures are treated as SENDIG 3.1. |
| CORE-000478 | negative/01, positive/01 | `Requested rule CORE-000478 does not match standard filter SENDIG 3.0` | `rule.yml` notes that the rule is not applicable to SENDIG-3.0, while the fixtures are SENDIG 3.0. |

## Review request

- Confirm whether the CORE-000217 `/05` fixtures should use SDTMIG/TIG standard
  metadata, or whether the rule authority should include the observed SENDIG
  fixture scope.
- Confirm whether CORE-000478 SENDIG 3.0 fixtures should be removed/retagged or
  whether the applicability note should be revised.

Until reconciled, these cases should remain report-only and excluded from
supported accuracy.
```
