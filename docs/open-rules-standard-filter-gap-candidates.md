# Open Rules Standard Applicability Gap Candidates

Snapshot: `target/open-rules-scoreboard-upstream-v31/scoreboard.json`

These cases are classified as `standard_filter_oracle_gap` and excluded from
supported accuracy. They are skipped because the fixture standard/version does
not match the rule authority or an applicability note in `rule.yml`.

They should not be forced into native execution until upstream confirms whether
the fixture standard metadata or rule applicability should change.

## Candidate Cases

| Rule | Cases | Candidate skip | Review finding | Suggested upstream request |
|---|---|---|---|---|
| `CORE-000217` | `negative/05`, `positive/05` | `Requested rule CORE-000217 does not match standard filter sendig 3.1` | The rule authority covers SDTMIG/TIG, while the `/05` fixtures are treated as SENDIG 3.1. The other eight CORE-000217 cases are already `supported_match`. | Confirm whether the `/05` fixtures should use SDTMIG/TIG standard metadata, or whether the rule authority should include the observed SENDIG fixture scope. |
| `CORE-000478` | `negative/01`, `positive/01` | `Requested rule CORE-000478 does not match standard filter SENDIG 3.0` | `rule.yml` explicitly notes that the rule is not applicable to SENDIG-3.0, while the fixtures are SENDIG 3.0. | Confirm whether the SENDIG 3.0 fixtures should be removed/retagged or whether the applicability note should be revised. |

## Local Policy

- Keep these cases report-only and warning-level in full upstream scoring.
- Do not count them as `skipped_unsupported`; the skip is a reviewed standard
  applicability conflict, not a generic engine gap.
- Do not promote them to `supported_match` unless upstream standard metadata or
  rule applicability is reconciled.
- If upstream changes the fixture metadata, rerun the full scoreboard and update
  `tests/open_rules/upstream-baseline.json`.

A copy-pasteable upstream issue draft is maintained in
[`open-rules-upstream-issue-drafts.md`](open-rules-upstream-issue-drafts.md).
