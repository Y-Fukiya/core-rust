# Open Rules Official Fixture Gap Candidates

Snapshot: `target/open-rules-scoreboard-upstream-v25/scoreboard.json`

These cases are currently classified as `official_oracle_fixture_gap` and are
excluded from supported accuracy. They should not be converted into native
engine compatibility hacks without upstream evidence, because the observed
official oracle and candidate output disagree in ways that may reflect fixture,
oracle, or rule-text drift.

## Summary

| Metric | Count |
|---|---:|
| Candidate cases | 47 |
| Distinct rules | 39 |
| Supported gate impact | 0 |
| Recommended treatment | Upstream issue/PR evidence backlog |

## Candidate Cases

| Rule | Case | Official issues | Candidate issues | Initial triage |
|---|---|---:|---:|---|
| `CORE-000014` | positive/02 | 3 | 0 | Positive fixture has official issues; review oracle/data alignment. |
| `CORE-000049` | negative/01 | 1 | 4 | Issue cardinality differs; review row scope and official expected rows. |
| `CORE-000080` | negative/01 | 0 | 6360 | Official empty vs large candidate output; unsafe to normalize away. |
| `CORE-000081` | negative/01 | 0 | 3232 | Official empty vs large candidate output; unsafe to normalize away. |
| `CORE-000108` | negative/02 | 0 | 2 | Official empty vs candidate issues; review rule applicability. |
| `CORE-000117` | negative/01 | 17 | 10 | Empty/non-empty family; official/candidate cardinality differs. |
| `CORE-000117` | negative/02 | 53 | 34 | Empty/non-empty family; official/candidate cardinality differs. |
| `CORE-000143` | positive/02 | 0 | 1 | Positive fixture candidate issue; review applicability before supporting. |
| `CORE-000172` | negative/05 | 0 | 1 | Reference distinct edge case; official empty vs candidate issue. |
| `CORE-000172` | positive/05 | 1 | 0 | Positive fixture has official issue; review oracle/data alignment. |
| `CORE-000184` | negative/02 | 12 | 22 | Cardinality differs; review row scope. |
| `CORE-000195` | negative/01 | 28 | 30 | Domain placeholder column-ref family; small cardinality drift. |
| `CORE-000197` | negative/02 | 45 | 51 | Domain placeholder column-ref family; small cardinality drift. |
| `CORE-000198` | negative/02 | 18 | 51 | Domain placeholder column-ref family; cardinality drift. |
| `CORE-000224` | negative/02 | 1 | 0 | Official issue absent in candidate; review official expectation. |
| `CORE-000225` | negative/01 | 8 | 6 | Cardinality differs; review row scope. |
| `CORE-000252` | negative/02 | 0 | 2 | Official empty vs candidate issues; review rule applicability. |
| `CORE-000262` | negative/01 | 0 | 4 | Official empty vs candidate issues; review rule applicability. |
| `CORE-000267` | negative/01 | 6 | 12 | Cardinality differs; review row scope. |
| `CORE-000268` | negative/02 | 12 | 22 | Cardinality differs; review row scope. |
| `CORE-000289` | negative/01 | 4 | 0 | Official issues absent in candidate; review official expectation. |
| `CORE-000370` | negative/01 | 9 | 8 | Small cardinality drift; review locator semantics. |
| `CORE-000438` | negative/01 | 1 | 2 | Cardinality differs; review row scope. |
| `CORE-000438` | positive/01 | 1 | 0 | Positive fixture has official issue; review oracle/data alignment. |
| `CORE-000454` | negative/02 | 0 | 3 | Official empty vs candidate issues; distinct operation review. |
| `CORE-000458` | negative/02 | 33 | 24 | Cardinality differs; review row scope. |
| `CORE-000529` | negative/01 | 28 | 24 | DY operation family; official includes questionable empty rows. |
| `CORE-000542` | negative/01 | 14 | 26 | Numeric placeholder conversion/requiredness family. |
| `CORE-000546` | positive/01 | 6 | 0 | Positive fixture has official issues; review oracle/data alignment. |
| `CORE-000554` | negative/01 | 2 | 0 | Official issues absent in candidate; review official expectation. |
| `CORE-000569` | negative/01 | 3 | 2 | Variable metadata requiredness/regex interpretation differs. |
| `CORE-000570` | negative/01 | 2 | 0 | Official issues absent in candidate; review official expectation. |
| `CORE-000648` | negative/01 | 33 | 2 | Empty/non-empty family; official/candidate cardinality differs sharply. |
| `CORE-000648` | positive/01 | 0 | 2 | Positive fixture candidate issues; review applicability before supporting. |
| `CORE-000698` | negative/01 | 36 | 24 | Cardinality differs; likely scope/row identity review. |
| `CORE-000698` | negative/02 | 12 | 20 | Cardinality differs; likely scope/row identity review. |
| `CORE-000698` | positive/01 | 0 | 4 | Positive fixture candidate issues; review applicability before supporting. |
| `CORE-000704` | negative/01 | 36 | 24 | Cardinality differs; likely scope/row identity review. |
| `CORE-000704` | negative/02 | 20 | 12 | Cardinality differs; likely scope/row identity review. |
| `CORE-000704` | positive/01 | 0 | 4 | Positive fixture candidate issues; review applicability before supporting. |
| `CORE-000718` | negative/01 | 0 | 8 | Official empty vs candidate issues; review rule applicability. |
| `CORE-000718` | positive/01 | 12 | 0 | Positive fixture has official issues; review oracle/data alignment. |
| `CORE-000750` | negative/01 | 72 | 72 | Same cardinality but identity/locator differs; split-domain uniqueness review. |
| `CORE-000770` | negative/01 | 15 | 12 | Distinct operation family; cardinality differs. |
| `CORE-000814` | negative/01 | 24 | 48 | USDM join operation overreports; join cardinality/filter review. |
| `CORE-000865` | negative/01 | 4 | 2 | Cardinality differs; review row scope. |
| `CORE-000960` | negative/01 | 80 | 75 | XHTML operation family; structural/flattening review. |

## Upstream Issue Template

For each proposed upstream issue or PR, include:

- Rule id and case id.
- The exact official `results.csv` row count.
- The candidate structural issue count from core-rust v25.
- A small excerpt of the input rows that demonstrate the discrepancy.
- Whether the discrepancy is cardinality, positive-fixture issue presence,
  official-empty/candidate-nonempty, or locator identity.
- A note that the case remains excluded from supported accuracy until upstream
  oracle/data are reconciled.
