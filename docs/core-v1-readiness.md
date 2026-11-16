# Official CORE v1.0 and core-rust readiness

Reviewed on 2026-11-16. This includes source inspection, a parser audit and a
core-rust run against candidate fixtures, not a v1.0 conformance certificate.
The official engines were not executed in this audit.
The accepted core-rust upstream lock and scoreboards are unchanged.

## Which CORE is the reference?

Official CORE v1.0 has two execution paths. They must not share an unqualified
"CORE v1" compatibility claim.

| Rule family | Official reference engine | Rule source |
| --- | --- | --- |
| SDTMIG 3.2, 3.3, 3.4 and FDA Business Rules | Verisian PostgreSQL engine | core-contributor |
| TIG, USDM and SEND | CDISC engine | cdisc-open-rules |

The split is documented in the
[Open Rules repository](https://github.com/cdisc-org/cdisc-open-rules/blob/1881cb9a1f9a38b6cafc785eca5169b240981f66/README.md).
Its SEND-only fixtures are not fully validated. SDTM-only rules in `Deprecated/`
are not a substitute for the new SDTM package.

The [Verisian v1.0.0 release](https://github.com/verisianHQ/cdisc-rules-engine/releases/tag/v1.0.0)
uses PostgreSQL and can create a temporary database for each run. It includes
rules, terminology and standards metadata. ADaM and Define-XML rules are still
in development. This describes the official engine, not core-rust capabilities.

The [CDISC v1.0.0 release](https://github.com/cdisc-org/cdisc-rules-engine/releases/tag/v1.0.0)
adds operators and operation parameters. A matching rule ID alone does not
establish that core-rust implements the revised condition or operation.

On the SQL path, the
[rule query compiler](https://github.com/verisianHQ/cdisc-rules-engine/blob/4a734105540a83f1df80d061df5049bd4b6bda4d/cdisc_rules_engine/compiler/compiler.py)
turns a rule and dataset into a SELECT query. Conditions determine which rows
fail; joins and operation outputs supply context. Record, group and dataset
sensitivity change which failing rows are reported. The result cap can differ
from the total error count. An adapter must preserve these distinctions instead
of treating every JSON array length as the total number of violations.

## Revisions inspected

| Component | Revision | Meaning |
| --- | --- | --- |
| Verisian engine | `4a734105540a83f1df80d061df5049bd4b6bda4d` | v1.0.0 release tag |
| CDISC engine | `f3282a0d870bbdf57e326e9992cceea3ed0d9af5` | v1.0.0 release tag |
| cdisc-org/core-contributor | `94963e01df448f05bdbbc43a482661d4f30f7dac` | Candidate rules snapshot inspected on review date |
| cdisc-org/cdisc-open-rules | `1881cb9a1f9a38b6cafc785eca5169b240981f66` | Candidate rules snapshot inspected on review date |
| core-rust | `fa99a2be857a7f40c0c9bf8d4b2dc0c2ec27fcf0` | Parser implementation audited |
| Accepted Open Rules lock | `7f7fae49376b3d023563ebb6c36a3b392d6e649f` | Existing regression corpus, not updated here |

The candidate rules snapshots are not claimed to be the exact packages bundled
with the two release tags. The contributor snapshot points to engine submodule
`63985f28bf87166adc5bcf90c012b7690aa47ba8`, not the Verisian v1.0.0 tag.
Adoption needs an explicitly verified engine/rules/cache combination.

## Package and report boundaries

The existing Open Rules harness discovers `Published/**/rule.yml` and compares
case `results.csv` with core-rust output. It executes core-rust; it does not run
the official engine merely because the corpus is official.

The inspected contributor package instead contains `sdtm_rules/<rule-id>/*.yml`,
with arbitrary YAML filenames, Excel test data and `results/results.json`.
There are 621 rule definitions, 1,544 JSON result files and no `results.csv`
files. These are different packaging and oracle contracts.

Contributor fixture JSON has a `datasets` array with dataset execution status,
`number_errors` and `errors`. It does not embed the rule ID. By contrast, the
official production JSON report uses `Issue_Details` and `Rules_Report`.
Its terminal statuses include `SUCCESS`, `ISSUE REPORTED`, `SKIPPED` and
`EXECUTION ERROR`, as defined by the pinned
[status enum](https://github.com/verisianHQ/cdisc-rules-engine/blob/4a734105540a83f1df80d061df5049bd4b6bda4d/cdisc_rules_engine/enums/execution_status.py)
and [report producer](https://github.com/verisianHQ/cdisc-rules-engine/blob/4a734105540a83f1df80d061df5049bd4b6bda4d/cdisc_rules_engine/services/reporting/base_report.py).
The raw `results_data` variant is another contract, not an empty issue list.

Do not rename a contributor result file to `report.json` and feed it to the
P21PORT comparison command. Do not reinterpret an unrecognized package as a
successful zero-rule build. A zero-finding pass requires recognized execution
evidence for the requested rule, not just the absence of recognized issues.

The boundary hardening rejects unknown report schemas, empty or wrong-rule
execution records, contradictory native summary counters and impossible CSV
aggregate-count distributions. An invalid JSON report cannot fall back to CSV
or become acceptable through `--allow-actual-skipped`. Recognized official
`SUCCESS` and native zero-issue `passed` records remain valid.

The Python package loader rejects contributor and mixed legacy/contributor
roots, ambiguous wrapper directories and zero-rule input. In a CLI reproduction,
the contributor CORE-000001 negative/01 JSON declared three errors. Supplying it
as a zero-issue comparison report used to pass; it now returns
`ACTUAL_INVALID_REPORT`. Building a catalog from the contributor root used to
succeed with zero CORE rules; it now stops with an unsupported-format error
before creating output. These are safety guards, not contributor support.

## Reproducible parser inventory

Run the example against a pinned, unpacked rules directory:

```sh
cargo run -p core-rule-model --locked --example audit_rule_package -- \
  /path/to/core-contributor/sdtm_rules > sdtm-parser-audit.json
cargo run -p core-rule-model --locked --example audit_rule_package -- \
  /path/to/cdisc-open-rules/Published > cdisc-parser-audit.json
```

The example calls the real Rust loader and walks its condition AST. It records
parse errors, duplicate rule IDs, multiple definitions in one directory,
unrecognized operators and rule types, and observed operation names. It skips
symlinks and positive/negative fixture subtrees. Output paths are relative.
Exit zero means the inventory was produced; inspect `counts.parse_errors` and
the unresolved collections. It is not a support or conformance gate.

| Snapshot scope | YAML files | Parsed | Parse errors | Rules with unrecognized operators | Rules with unrecognized types |
| --- | ---: | ---: | ---: | ---: | ---: |
| Contributor `sdtm_rules` | 621 | 619 | 2 | 43 | 30 |
| Open Rules `Published` | 823 | 823 | 0 | 100 | 0 |

Neither snapshot had duplicate parsed rule IDs. In the contributor package,
28 operator names and 9 rule-type names were unrecognized. Operator and type
counts overlap in two rules, so they identify 71 distinct parsed rules, not 73.
The parser rejected CORE-000133 for a repeated YAML `Item` key and CORE-000264
for quoted-scalar indentation. These are observations about the current parser;
they do not prove how the official engine handles those files.

Open Rules has 7 unrecognized operator names. Of the 100 affected rules, 96
contain `unsupported_jsonata`. Existing rule-specific hand ports may handle
some of these later in the pipeline. This parser-only inventory does not run
those paths and must not be reported as 100 execution failures.

Likewise, a recognized operator or observed operation name is not proof of
correct execution. This audit does not inspect every operation parameter,
dictionary, codelist, metadata dependency or output locator convention.

## Candidate fixture run

The same core-rust revision executed all 2,298 discovered `Published` cases in
the pinned Open Rules candidate checkout. These are case counts, not rule
counts. The run produced reports for every case with no harness execution
error. It did not execute the Verisian SDTM package or either official engine.

| Bucket | Default scoring | Strict scoring |
| --- | ---: | ---: |
| Supported match | 2,070 | 1,232 |
| Supported mismatch | 84 | 965 |
| Deferred oracle-gap mismatch | 25 | 0 |
| Deferred oracle-gap skipped | 52 | 0 |
| Skipped unsupported | 55 | 89 |
| No official oracle | 12 | 12 |
| Harness error | 0 | 0 |

Both scoring gates failed. Of the default unsupported cases, 40 report
`operations_not_supported`, 9 `unsupported_rule_type`, 5 `unsupported_operator`
and 1 `dataset_join_not_supported`. Operation candidates include cross-domain
`distinct`, `referenced_domain_variable_names`, `intersect` and
`get_codelist_attributes`; their parameters and scope need separate tests.

Strict scoring moves 838 default supported matches to mismatch. This is not
evidence of 838 different engine bugs. For example, CORE-000001 negative/01 has
the same nine variable-level findings, but the official CSV lacks USUBJID/SEQ
values present in the candidate. Default scoring aligns those identities;
strict scoring does not. The existing `scoring_normalizations` trace does not
record that base alignment, so its counts alone understate the difference.
Use the delta's changed cases and both scoreboards, not that count or the
`strict_identity` label alone, when assessing raw agreement. Complete alignment
tracing remains a separately reviewed baseline-metadata change.

The macOS checkout reported case-colliding `Results/results.csv` and
`results/results.csv` paths for CORE-000749 positive/01. Both Git entries have
the same blob `a13018c5f063f58a0f793c5939e5388f2aba7345`; discovery uses one case.
A case-sensitive checkout should still be checked before adoption.

To reproduce the diagnostic run, use a clean checkout at the candidate SHA and
fresh output directories. The commands deliberately omit `--strict-lock` because
the candidate is not the accepted lock. A lock mismatch warning and nonzero
scoring exit status are expected; do not use this run to replace accepted gates.

```sh
cargo run -p xtask --locked -- open-rules run-score \
  --open-rules-root /path/to/pinned-cdisc-open-rules --scope Published \
  --core-rs-results-root target/core-v1-candidate \
  --out target/core-v1-default
cargo run -p xtask --locked -- open-rules score \
  --open-rules-root /path/to/pinned-cdisc-open-rules --scope Published \
  --core-rs-results-root target/core-v1-candidate \
  --out target/core-v1-strict --strict-scoring
cargo run -p xtask --locked -- open-rules score-delta \
  --default-scoreboard target/core-v1-default/scoreboard.json \
  --strict-scoreboard target/core-v1-strict/scoreboard.json \
  --out target/core-v1-delta
```

## Remaining adoption gates

The [candidate audit](open-rules-candidate-audit.md) now records a fixed CDISC
diagnostic profile, successful Linux reproduction, official representative
execution, and an exhaustive 109-case mismatch inventory. It does not promote
the candidate. The fresh official CORE-000699 result differs from the stored
fixture, so reference-version alignment still needs review before adoption.

1. Pin the reference engine, rule package, CT/metadata cache and standard/version
   together. Keep CDISC and Verisian evidence separate.
2. Add a contributor package adapter with explicit rule-to-case association,
   Excel schema/missing-value handling and JSON oracle normalization. Cover
   positive, negative, skipped and execution-error cases independently.
3. Run the pinned official engine on representative fixtures before treating
   stored fixture expectations as verified reference output. Capture its actual
   version, command, input hashes and reports.
4. Address parser and engine gaps in separate changes. Start with bounded
   operator families; keep external dictionary licensing and availability
   explicit. Do not add oracle-gap exemptions to improve headline coverage.
5. Run default and strict full-corpus scoring for each engine/package profile,
   inspect structural differences and only then propose new accepted baselines.

The earlier September candidate mismatch counts belong to a different snapshot.
They are not the v1.0 results. Current accepted regression metrics likewise do
not establish CORE v1.0 compatibility.
