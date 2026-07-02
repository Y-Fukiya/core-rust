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
| `deferred_oracle_gap_mismatch` | Candidate ran and differs from the official oracle, but the rule family is explicitly deferred as an oracle-semantics gap pending native verification. It is not a skipped execution and is excluded from supported accuracy. | non-zero |
| `deferred_oracle_gap_skipped` | Candidate skipped execution for a rule family that is explicitly deferred as an oracle-semantics gap, or the case has a reviewed official oracle/data fixture inconsistency that makes the official result unsuitable as a correctness oracle. It remains a coverage gap, but it is not counted as generic unsupported coverage. | zero for score, review-required in baseline when increased |
| `skipped_unsupported` | Candidate output contains skipped rows. | non-zero |
| `mixed_skipped_and_issues` | Candidate output mixes skipped rows and issue rows for the same case. | non-zero |
| `no_official_oracle` | The case has no official `results.csv`; it is retained for accounting and excluded from supported accuracy. | zero |
| `harness_error` | Official or candidate report is missing, malformed, or cannot be scored. | non-zero |

Skipped and wrong are separate. Skipped cases are coverage gaps. Supported
mismatches are correctness problems. Deferred oracle-gap buckets preserve the
evidence separately: mismatches stay as mismatches, and reviewed oracle-gap
skips stay as skips without being counted as generic `skipped_unsupported`.

Reviewed official fixture gaps are also reported as
`deferred_oracle_gap_skipped`. These are cases where the committed official
`results.csv` is inconsistent with the rule or input data, such as an official
result referring to a variable/value absent from the fixture. They are not
supported matches and must not be used as native engine conformance evidence.
The current full-upstream inventory is maintained in
`docs/open-rules-deferred-gap-inventory.md`.

As of the v31 upstream scoreboard, the remaining 55
`deferred_oracle_gap_skipped` cases are split into:

- 51 `official_oracle_fixture_gap` cases, tracked as upstream oracle/data review
  candidates rather than native engine implementation backlog. See
  `docs/open-rules-official-fixture-gap-candidates.md`.
- 4 `standard_filter_oracle_gap` cases, where the fixture standard/version does
  not match the rule authority or applicability note. See
  `docs/open-rules-standard-filter-gap-candidates.md`.

There are no remaining `required_value_metadata` or `defer_distinct_operation`
skipped cases in the v31 inventory. Those previously ambiguous cases were
reclassified after targeted review showed that returning them to supported
scoring would either contradict the official oracle or create a deferred
mismatch.

For reviewed row-locator oracle-gap families, scoring may ignore record locator
fields (`row`, `usubjid`, and `seq`) while still comparing rule id, dataset,
domain, variables, and multiset counts. This is limited to manifest-backed
families where official and candidate issue identity is known to differ only by
unstable row location; it must not hide missing or extra issue counts.

For reviewed `empty/non_empty` oracle-gap families, scoring may drop candidate
output-context variables at the same rule/dataset/domain/row/subject/sequence
location when the official oracle reports only the failed condition variables.
This is limited to manifest-backed cases and does not ignore extra rows, missing
official variables, or different record locators.

The same output-context-variable normalization applies to reviewed
positive-zero probe oracle-gap families. It can remove extra candidate variables
only when the official oracle already has an issue at the same structural
location; candidate-only rows and official-only rows remain mismatches.

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
- `deferred_oracle_gap_mismatch`: official-oracle-backed mismatches deferred by
  an explicit oracle-gap policy; treat increases as review-required, not as
  coverage improvement.
- `deferred_oracle_gap_skipped`: official-oracle-backed skipped execution
  deferred by an explicit oracle-gap policy; treat increases as review-required,
  not as supported coverage.
- `native_engine_coverage`: share of all discovered cases covered by supported
  native engine execution.
- `rule_id_hand_port_coverage`: share of all discovered cases covered by
  supported rule-id hand-port execution.
- `no_official_oracle`: cases retained for accounting but excluded from
  supported accuracy.

The synthetic oracle counters remain in the JSON schema for older scoreboard
compatibility, but current scoring should leave them at zero.

`summary.md` includes an `Execution Provenance` section. Read it before using
aggregate supported accuracy as an engine-capability claim: native engine and
rule-id hand-port support are intentionally reported separately. `summary.md`
also includes a `Skipped Unsupported Reasons` section when skipped cases have
`skipped_reason` values. Use that section as the first coverage triage list
before promoting more cases into supported coverage.

## Metrics

```text
supported_accuracy = supported_match / (supported_match + supported_mismatch)
coverage = (supported_match + supported_mismatch) / total_cases
official_coverage = official_oracle_match / total_cases
native_engine_coverage =
  (native_engine_supported_match + native_engine_supported_mismatch) /
  total_cases
rule_id_hand_port_coverage =
  (rule_id_hand_port_supported_match + rule_id_hand_port_supported_mismatch) /
  total_cases
```

Coverage can be low while supported accuracy is high. That means the roadmap is
large, not that supported behavior is wrong.

`coverage` is now official-oracle-backed coverage because missing-official cases
are excluded from the supported numerator.

Aggregate `coverage` includes both native engine and rule-id hand-port supported
cases. Use `native_engine_coverage` when describing generic engine support.

Rule-id hand-port provenance is driven by
`crates/core-api/src/open_rules_compat/hand_port_rule_ids.csv`, not by an inline
Rust `matches!` list. Treat that manifest as an Open Rules oracle-harness
compatibility policy file: entries should be reviewed like coverage exceptions,
not like generic engine semantics.

Open Rules oracle-gap rule-id membership is similarly driven by
`crates/core-api/src/open_rules_compat/oracle_gap_rule_ids.csv`. Each row carries
the rule id, gap category, reason, owner, evidence source, and scope. The Rust
loader validates the header, exact row count, CORE id format, category, reason,
owner, evidence, scope, duplicate headers, and duplicate rule ids within the
same category. The engine code should keep semantic predicates in Rust and use
this manifest only for reviewed rule-id membership.

Reference-distinct oracle gaps are split into narrower scoreboard families when
the remaining mismatch is not safe to normalize away: official-empty oracles,
fixture row locators that point outside the loaded data rows, broad cardinality
differences, and scope-wide distinct behavior. These categories are triage
labels for future native semantics work, not supported matches.

Remaining hard-coded `CORE-xxxxxx` references in `core-api` are inventoried in
`crates/core-api/src/open_rules_compat/rule_specific_semantics.csv`. That file
does not make the rules generic; it classifies why each rule-specific reference
still exists, such as USDM JSONata hand-port semantics, metadata adapters,
standard-filter compatibility, or result post-processing. Unit tests scan the
core API source files and fail when a new hard-coded CORE id appears without a
classification row.

USDM JSONata hand-port semantics are isolated in
`crates/core-api/src/usdm_jsonata.rs`. `core-api/src/lib.rs` should call that
module, not carry inline USDM rule-family lists.

Remaining Open Rules engine-semantics rule-id membership is isolated in
`crates/core-api/src/engine_semantics.rs`. `core-api/src/lib.rs` should use
those classification helpers instead of inline `CORE-xxxxxx` literals.

The full upstream regression baseline lives in
`tests/open_rules/upstream-baseline.json`. It is generated from the pinned
upstream SHA in `tests/open_rules/upstream.lock`, with local filesystem paths
normalized and per-case issue diff arrays stripped so the baseline remains
portable. Stripped baselines keep `missing_count`, `extra_count`, and
`issue_fingerprint_hash`, and the comparator uses those portable fields before
falling back to full arrays. This preserves same-bucket regression detection
without requiring full issue arrays in the committed upstream baseline. The
upstream regression workflow treats scoreboard generation failure as an
infrastructure failure, then lets the baseline comparison decide whether known
compatibility gaps have regressed.

A supported case moving from `rule_id_hand_port` provenance to `native_engine`
provenance is not counted as an automatic baseline improvement. The baseline
command reports that transition as `review-required` so reviewers can confirm
that the rule-specific execution path was actually retired or replaced by
generic engine semantics. `review-required` differences fail the baseline gate
until the transition is explicitly reviewed and the accepted baseline is
updated.

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
Supported matches moving from `rule_id_hand_port` provenance to
`native_engine` provenance are reported as `review-required`, not automatic
improvements, because manifest removal alone is not evidence that generic
engine semantics replaced rule-specific behavior. `review-required` entries
make the baseline command exit non-zero in CI.
Unknown provenance transitions are reported neutrally unless they also change
the score bucket. Provenance-related differences include the old and new
provenance values in the baseline report output.

The run-score command exits non-zero for correctness failures, harness failures,
or unresolved unsupported execution: `supported_mismatch > 0`,
`deferred_oracle_gap_mismatch > 0`, `skipped_unsupported > 0`,
`harness_error > 0`, or `mixed_skipped_and_issues > 0`.
`deferred_oracle_gap_skipped` and `no_official_oracle` remain reportable
coverage/oracle gaps, but they do not fail the standalone score command.
Use `--fail-on-deferred-oracle-gap` when a standalone score command should also
fail on any `deferred_oracle_gap_skipped` case.

CI runs the repository-local executable fixture only. It does not download or
vendor the full upstream `cdisc-open-rules` repository, so normal pull requests
are not blocked by network access or upstream drift.

The full upstream oracle run is fixed as a separate GitHub Actions workflow,
`Open Rules Upstream`. It can be started manually with `workflow_dispatch` and
also runs weekly. That workflow checks out the pinned SHA from
`tests/open_rules/upstream.lock`, runs `xtask open-rules run-score` with
`--strict-lock`, and uploads the upstream scoreboard artifacts.

The committed full upstream baseline is a default-engine regression baseline.
`ValidateRequest::open_rules_oracle_compat` remains an explicit API/test-only
compatibility switch; the upstream workflow does not silently enable it. Treat
`tests/open_rules/upstream-baseline.json` as a "do not get worse" guard for the
default engine plus tracked hand-port provenance, not as a full conformance
certificate.

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
