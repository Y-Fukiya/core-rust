# Open Rules candidate audit

This diagnostic fixes comparison conditions and classifies candidate mismatches.
It does not adopt new rules, change the accepted baselines, or change engine or
scorer behavior. The accepted lock remains
`7f7fae49376b3d023563ebb6c36a3b392d6e649f`.

## Fixed inputs

The machine-readable profile is
[`cdisc-v1-candidate.json`](../tests/open_rules/diagnostic/cdisc-v1-candidate.json).

| Input | Pin |
| --- | --- |
| Candidate CDISC Open Rules | `1881cb9a1f9a38b6cafc785eca5169b240981f66` |
| Official CDISC engine v1.0.0 | `f3282a0d870bbdf57e326e9992cceea3ed0d9af5` |
| Linux release | `core-ubuntu-22.04.tar.gz` |
| Release SHA-256 | `52a932ca3f6cbc08055da2611c2a37badde78ee1f426999d39d4cbb24467d870` |
| Complete 222-file cache map SHA-256 | `9b0d65304dced1746f32d5b3f5a7225519925746a9405db3ef3fd10a6b748c8d` |
| Execution host | Ubuntu 22.04, x86-64, case-sensitive filesystem |
| Scope | Published, 2,298 discovered cases |

The official release archive fixes its bundled Python dependencies, metadata,
terminology, and schema caches. The audit never updates that cache. Individual
fixture inputs, environment files, rule YAML, command arguments, and report files
are hashed or recorded in the official-run artifact. Standard, version,
substandard, and use case come from each fixture unless a separately named
diagnostic variant explicitly overrides them. No CT package is silently replaced
with the latest available package.

The verified Linux archive stores `core/core` with mode `0644`. After verifying
the archive digest, the workflow adds owner execute permission without changing
the binary bytes. Public fixture `.env` snapshots are included in the official
artifact; the runner rejects environment keys outside the comparison settings.
The binary runs from its extracted `core` directory because report generation
opens `resources/templates/report-template.xlsx` relative to the working
directory. Outputs and fixture snapshots use separate absolute paths.

This is a **diagnostic pair**, not the original packaged combination. The rules
snapshot's engine gitlink is `42028faf090c59bfdfba974779f64be2448bf5bb`, which differs
from the v1.0.0 tag. Local YAML execution also differs from selecting rules in the
release's shipped rules cache. Legacy SDTM fixtures executed by this CDISC engine
do not establish conformance with the separate Verisian SDTM engine.

The core-rust side uses the unchanged `xtask open-rules run-score` path. It reads
fixture `.env` standard/version and Open Rules data-directory metadata. It does
not load the official Python cache. These are recorded comparison conditions,
not a claim that both engines implement identical metadata or CT lookup behavior.

## Linux evidence

Manually dispatch **Open Rules Upstream** with profile `cdisc-v1-candidate`.
The default profile is `accepted`. Scheduled runs still execute the accepted
observe and regression jobs. Candidate jobs cannot update the accepted lock or
baseline.

The candidate job requires a case-sensitive filesystem and checks that both
CORE-000749 positive/01 oracle paths exist with identical contents. It records the
core-rust commit, rules commit, profile hash, runner image, Rust version, and
protected baseline hashes. A nonzero scoring exit is retained as a diagnostic
result; missing scoreboards, harness errors, or failed execution fail the job.

Archive both candidate artifacts from the same workflow run and attempt:

- `open-rules-candidate-linux-<run>-<attempt>` contains conditions, candidate CSVs,
  run-summary, default scoreboard, strict scoreboard, delta JSON and Markdown,
  and exhaustive mismatch triage.
- `open-rules-candidate-official-<run>-<attempt>` contains the fixed profile,
  official invocation records, input and cache hashes, stdout/stderr, reports,
  and execution outcomes.

Official process exit zero is not proof that a rule ran. The report must identify
the requested rule and its execution status. Skipped, execution error, missing
report, valid zero-finding execution, and execution with findings stay separate.
CSV row counts have a different unit from input records or rule execution counts.

## Family classification

`scripts/open_rules_candidate_triage.py` reads paired scoreboards and the pinned
rule YAML. It does not run either engine or rewrite comparison keys. It rejects
duplicate case identities, unmatched case sets, inconsistent summary counts,
and incompatible upstream identities.

Each default mismatch has one primary family plus overlapping traits such as
rule type, sensitivity, operations, operators, and declared output variables.
Supported mismatches and deferred mismatches have separate counts. Unsupported
skips and strict-only changes are reported separately, not added to the mismatch
denominator.

Family membership is not a root-cause verdict. Dataset and Group sensitivity can
explain differences in reporting granularity, but each proposed correction still
needs rule-text and actual-report review. Scoreboard missing/extra keys are
scorer-produced evidence, not raw engine output. Strict mode disables the main
oracle-informed alignment policies but still uses the shared CSV parser and its
rule-specific transformations.

Reproduce triage offline with the matching candidate checkout and downloaded
scoreboards in fresh output directories:

```sh
python scripts/open_rules_candidate_triage.py \
  --default-scoreboard candidate-audit/default/scoreboard.json \
  --strict-scoreboard candidate-audit/strict/scoreboard.json \
  --rules-root ../cdisc-open-rules \
  --out target/candidate-triage-review
```

Adoption remains a separate change after the classified discrepancies are
resolved or supported by reviewed upstream evidence.

## Reproduced mismatch inventory

The first successful [Linux candidate job](https://github.com/Y-Fukiya/core-rust/actions/runs/43898049724)
executed all 2,298 cases. Its case keys, buckets, official/candidate issue counts,
missing/extra counts, and issue fingerprints match the earlier macOS run with
zero differences. This run's official-engine job failed preflight; its success
must not be inferred from the candidate job.

The [portable mismatch inventory](open-rules-candidate-mismatches.csv) contains
109 cases across 82 rules. Families use the declared precedence in the classifier,
so totals are disjoint. Other traits can overlap.

| Primary family | Supported mismatch | Deferred mismatch | Total |
| --- | ---: | ---: | ---: |
| Dataset sensitivity | 26 | 5 | 31 |
| Record comparator | 24 | 3 | 27 |
| Date | 5 | 7 | 12 |
| Domain presence | 9 | 0 | 9 |
| Distinct operation | 3 | 4 | 7 |
| Relationship | 6 | 1 | 7 |
| Empty/existence | 6 | 0 | 6 |
| Variable metadata | 3 | 1 | 4 |
| Group sensitivity | 0 | 3 | 3 |
| Other operation | 2 | 0 | 2 |
| String | 0 | 1 | 1 |
| Total | 84 | 25 | 109 |

There are no unclassified cases within these 109 mismatches. This does not mean
every family in the entire corpus is covered by the classifier. The 55 default
unsupported skips, 52 deferred skips, 12 missing-oracle cases, and 838 default-match
to strict-mismatch transitions remain separate populations.

## Official rerun observations

The final [diagnostic run 43900071480](https://github.com/Y-Fukiya/core-rust/actions/runs/43900071480)
passed both candidate and official jobs. Its engine version was `1.0.0` and its
cache map matched the fixed digest. The 22 invocations comprise 17 original
fixture profiles, three explicitly named TIG variants, and two status probes.
Nine reported SUCCESS, eleven ISSUE REPORTED, one expected SKIPPED, and one
expected EXECUTION ERROR. The last two prove status handling, not conformance.

Of the 17 original-profile runs, 16 CSV multisets exactly match the stored
fixtures, including values and multiplicity. CORE-000699 negative/01 differs.
Do not count the three TIG variants or two probes as confirmations of the
original fixture oracle. Per-run observations are in the
[official comparison inventory](open-rules-candidate-official.csv).

| Case | Stored CSV rows | Fresh official CSV rows | core-rust scored issues | Follow-up |
| --- | ---: | ---: | ---: | --- |
| CORE-000013 negative/01 | 1 | 1 | 13 | Dataset reporting granularity |
| CORE-000142 negative/01 | 10 | 10 | 12 | Group keys and reporting granularity |
| CORE-000699 negative/01 | 399 | 98 | 231 | Reference-version discrepancy first |
| CORE-000376 negative/02 | 6 | 6 | 48 | Metadata scope and output multiplicity |
| CORE-000590 negative/01 | 1 | 1 | 1 | Structural identity, not count |
| CORE-000016 negative/01 | 68 | 68 | 68 | Native TIG record-level control |

CSV data rows and scored issue keys are different units. These observations
prioritize investigation; they do not establish the cause of every family.
CORE-000013 is a strong first candidate because the rule declares Dataset
sensitivity and both fresh official runs and stored fixtures emit one dataset
finding. The classifier does not silently collapse core-rust's 13 records.
CORE-000699 must not be forced to 399 based only on the stored CSV: the selected
official v1 engine itself emits 98 under the same fixture environment.

The final run uses source commit `9b09429503d48956472e98f59212ec4877cf3b35`.
Its artifacts are preserved locally under
`target/core-v1-audit-20261117/run-43900071480/` and linked from the workflow.
Artifact ZIP SHA-256 is checked when downloading. Earlier failed runs retain
their failure status; no missing report was treated as a zero-finding result.

Next, implement a bounded Dataset sensitivity regression separately from the
audit tooling. Then investigate Group and metadata output scope. Resolve the
CORE-000699 reference-version discrepancy before any baseline promotion.
The accepted lock, baselines, scorer, and validation engine are unchanged.
