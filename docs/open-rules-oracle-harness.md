# CDISC Open Rules Oracle Harness

`core-rust` treats `cdisc-org/cdisc-open-rules` as an oracle-backed
compatibility corpus. Each case is a combination of `rule.yml`, test data under
`data/`, and committed official `results/results.csv`.

The harness can score existing core-rust `report.csv` files or run core-rust
against Open Rules cases before scoring. The comparison is structural and does
not use diagnostic message text as a primary correctness key.

## Command

Score reports that already exist:

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard
```

Run core-rust and write candidate reports:

```sh
cargo run -p xtask -- open-rules run \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs
```

Run and score in one command:

```sh
cargo run -p xtask -- open-rules run-score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard
```

The default scope is `Published`. Add `--scope Unpublished` to include another
scope.

Add `--strict-lock` to `run-score` when a local full-corpus run must fail if the
checkout SHA differs from `tests/open_rules/upstream.lock`.

Compare a scoreboard against the accepted baseline:

```sh
cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

## Candidate Report Layout

Candidate reports must mirror the official case identity:

```text
<core-rs-results-root>/<scope>/<rule_id>/<case_kind>/<case_id>/report.csv
```

Example:

```text
target/open-rules-core-rs/Published/CORE-000001/negative/01/report.csv
```

## Buckets

| Bucket | Meaning | Command exit |
|---|---|---|
| `supported_match` | Candidate ran and normalized issue keys match the official oracle. | zero |
| `supported_mismatch` | Candidate ran but normalized issue keys differ. | non-zero |
| `skipped_unsupported` | Candidate output contains skipped rows. | zero |
| `mixed_skipped_and_issues` | Candidate output mixes skipped rows and issue rows for the same case. | non-zero |
| `no_official_oracle` | The case has no official `results.csv`; it is excluded from supported accuracy and fails the score gate until explicitly resolved. | non-zero |
| `harness_error` | Official or candidate report is missing, malformed, or cannot be scored. | non-zero |

Skipped and wrong are separate. Skipped cases are coverage gaps. Supported
mismatches are correctness problems.

## Missing Official Oracle Policy

Some upstream cases do not include official `results/results.csv`. The harness
classifies these cases as `no_official_oracle`, even when a candidate report
exists and appears plausible. Candidate output must not become its own oracle,
so missing-official cases are excluded from `supported_match`,
`supported_mismatch`, `supported_accuracy`, and `coverage`.

Read these fields together:

- `official_oracle_match`: supported matches backed by committed official
  `results.csv`.
- `supported_mismatch`: official-oracle-backed cases where structural issue keys
  differ.
- `no_official_oracle`: cases retained for accounting but excluded from
  supported accuracy.
- `native_engine_supported_accuracy`: accuracy for supported cases evaluated
  without known rule-id-specific execution rewrites.
- `native_engine_coverage`: share of all discovered cases covered by native
  engine supported cases.
- `rule_id_hand_port_supported_accuracy`: accuracy for supported cases whose
  executable semantics are hand-ported or adjusted by CORE rule id before
  engine execution.
- `rule_id_hand_port_coverage`: share of all discovered cases covered by
  rule-id hand-port supported cases.

The synthetic oracle counters remain in the JSON schema for older scoreboard
compatibility, but current scoring should leave them at zero.

`summary.md` also includes a `Skipped Unsupported Reasons` section when skipped
cases have `skipped_reason` values. Use that section as the first coverage
triage list before promoting more cases into supported coverage.

`summary.md` also includes an `Execution Provenance` section. Use it to keep
native operator-engine coverage separate from rule-id hand ports. A full
`supported_accuracy` claim is useful for regression tracking, but it should not
be presented as pure generic engine capability unless the native-engine
provenance counters support that claim.

Aggregate `coverage` includes both native engine and rule-id hand-port
supported cases. Use `native_engine_coverage` when describing generic engine
support.

## Metrics

```text
supported_accuracy = supported_match / (supported_match + supported_mismatch)
coverage = (supported_match + supported_mismatch) / total_cases
official_coverage = official_oracle_match / total_cases
native_engine_supported_accuracy =
  native_engine_supported_match /
  (native_engine_supported_match + native_engine_supported_mismatch)
native_engine_coverage =
  (native_engine_supported_match + native_engine_supported_mismatch) /
  total_cases
rule_id_hand_port_supported_accuracy =
  rule_id_hand_port_supported_match /
  (rule_id_hand_port_supported_match + rule_id_hand_port_supported_mismatch)
rule_id_hand_port_coverage =
  (rule_id_hand_port_supported_match + rule_id_hand_port_supported_mismatch) /
  total_cases
```

Coverage can be low while supported accuracy is high. That means the roadmap is
large, not that supported behavior is wrong.

`coverage` is now official-oracle-backed coverage because missing-official cases
are excluded from the supported numerator.

## Normalization

The harness compares structural issue keys:

- rule id
- dataset
- domain
- row
- variables
- USUBJID
- sequence value

It does not compare diagnostic messages. Message text is retained in source
reports but is not a primary correctness key.

## Schema-Aware Loading

Open Rules case execution uses a dedicated data loader for each case `data/`
directory. It reads `_datasets.csv` to find dataset CSV files and
`_variables.csv` as the schema authority.

Declared numeric variables are loaded as numeric values, so values such as
`001` compare as the number `1` when a rule expects numeric semantics.
Declared character variables remain strings. Existing generic CSV validation is
unchanged and continues to use the normal inference path.

The loader records warnings for undeclared CSV columns, declared variables that
are missing from the CSV, and invalid numeric cells. Invalid numeric cells are
loaded as null rather than panicking.

## Baseline And CI

`tests/open_rules/baseline.json` records the accepted repository-local fixture
state. The baseline command fails on regressions such as:

- `supported_match` becoming any other bucket.
- `supported_match` staying matched but regressing from `native_engine`
  provenance to `rule_id_hand_port` or `unknown` provenance.
- new `supported_mismatch` cases.
- new `harness_error` cases.
- baseline cases missing from the current scoreboard.

Improvements to `supported_match` are allowed and printed as improvements.
Supported matches moving from `rule_id_hand_port` or `unknown` provenance to
`native_engine` provenance are also reported as improvements.

The run-score command exits non-zero for correctness failures, harness failures,
or unresolved official oracle gaps: `supported_mismatch > 0`,
`harness_error > 0`, `no_official_oracle > 0`, or
`mixed_skipped_and_issues > 0`.

CI runs the repository-local executable fixture only. It does not download or
vendor the full upstream `cdisc-open-rules` repository, so normal pull requests
are not blocked by network access or upstream drift.

The full upstream oracle run is fixed as a separate GitHub Actions workflow,
`Open Rules Upstream`. It can be started manually with `workflow_dispatch` and
also runs weekly. That workflow checks out the pinned SHA from
`tests/open_rules/upstream.lock`, runs `xtask open-rules run-score` with
`--strict-lock`, and uploads the upstream scoreboard artifacts.

## Full Upstream Workflow

Use a separately checked out and reviewed `cdisc-open-rules` tree for local
full-corpus checks:

```sh
git clone https://github.com/cdisc-org/cdisc-open-rules.git ../cdisc-open-rules
cd ../cdisc-open-rules
git checkout 7f7fae49376b3d023563ebb6c36a3b392d6e649f
cd ../core-rust
cargo run -p xtask -- open-rules run-score \
  --open-rules-root ../cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard \
  --strict-lock
```

Upstream SHA bumps should be reviewed in their own PR so data changes and
engine changes do not blur together.

## Phase Roadmap

Phase 2 added `_variables.csv` schema-aware CSV loading through a dedicated Open
Rules data path.

Phase 3 runs core-rust against selected cases and writes candidate reports into
the mirrored layout.

Phase 4 adds baseline policy, strict upstream lock enforcement, and CI.
