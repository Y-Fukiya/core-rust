# Agent Guidance

## CDISC Open Rules Oracle Work

When working on `cdisc-open-rules` compatibility, treat
`cdisc-org/cdisc-open-rules` as an oracle-backed conformance corpus.

- Do not mix skipped and wrong. Skipped cases are coverage gaps; supported
  mismatches are correctness problems.
- Do not use diagnostic message text as a primary comparison key.
- Compare structural fields such as rule id, dataset/domain, row, variables,
  USUBJID, and sequence value.
- Keep Phase 1 read-only: discovery, normalization, scoring, and reports only.
- Do not change engine semantics in the same change as the Phase 1 harness.
- Keep `_variables.csv` type authority work in Phase 2.
- Use LLM-generated data only as a second layer after official oracle scoring is
  stable and official CORE output has confirmed expected results.
