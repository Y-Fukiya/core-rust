# CDISC Open Rules Oracle Harness Design

Date: 2026-06-19
Status: approved design

## Purpose

Improve core-rust compatibility evidence by treating `cdisc-org/cdisc-open-rules`
as an oracle-backed conformance corpus.

The first phase must be a read-only differential report harness. It compares
official committed `results.csv` files from `cdisc-open-rules` against already
generated core-rust `report.csv` files. It must not run core-rust, change engine
semantics, add schema-aware CSV loading, or apply baseline policy.

## Chosen Approach

Use an xtask-first design.

Add an `xtask` workspace crate with one public command:

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard
```

This keeps compatibility tooling separate from the user-facing `core-rs`
validate CLI while allowing normal Rust tests and CI integration.

## Scope

Phase 1 includes:

- `xtask` crate.
- `open-rules score` command.
- Case discovery from a local `cdisc-open-rules` checkout.
- Official and candidate CSV normalization.
- Case-level scoring.
- `scoreboard.json` and `summary.md` outputs.
- `tests/open_rules/upstream.lock`.
- Synthetic in-repository fixtures for tests.
- `docs/open-rules-oracle-harness.md`.
- Root `AGENTS.md` guidance for future coding agents.

Phase 1 excludes:

- Running core-rust against open-rules cases.
- `_variables.csv` schema-aware CSV loading.
- Baseline acceptance policy.
- GitHub Actions required checks.
- Engine semantic fixes.
- LLM-generated test data.

## Architecture

The `xtask` crate should be split into small modules:

- `main.rs`: clap CLI with only `open-rules score` exposed.
- `open_rules/discovery.rs`: scan `Published` by default and optional scopes
  passed via `--scope`.
- `open_rules/normalize.rs`: normalize official `results.csv` and core-rust
  `report.csv` rows into common issue keys.
- `open_rules/score.rs`: classify each case into one of the result buckets.
- `open_rules/report.rs`: write JSON and Markdown outputs.
- `open_rules/upstream.rs`: read local checkout SHA and
  `tests/open_rules/upstream.lock`; lock mismatch is a warning in Phase 1.

Only `score` is public in Phase 1. Discovery and normalization should be
separate internal functions with direct tests, so public subcommands can be
added later without restructuring.

## Data Flow

1. Read `--open-rules-root`.
2. Discover cases in `Published` by default. Additional scopes are included only
   when passed with `--scope`.
3. Identify each case by `scope`, `rule_id`, `case_kind`, and `case_id`.
4. Record paths and metadata for `rule.yml`, `.env`, `_datasets.csv`,
   `_variables.csv`, dataset files listed by `_datasets.csv`, and official
   `results/results.csv`.
5. Read official `results.csv`; missing official output is `harness_error`.
6. Read candidate `report.csv` from the mirrored layout:

```text
<core-rs-results-root>/<scope>/<rule_id>/<case_kind>/<case_id>/report.csv
```

7. Missing candidate output is `harness_error`.
8. Normalize official and candidate rows into issue sets.
9. If any candidate row is skipped, classify the whole case as
   `skipped_unsupported`.
10. Otherwise compare official and candidate issue sets.
11. Exact match is `supported_match`.
12. Any missing or extra issue key is `supported_mismatch`.
13. Write complete details to `scoreboard.json`.
14. Write a human-oriented `summary.md` that prioritizes mismatches and harness
   errors.
15. Return non-zero when any `supported_mismatch` or `harness_error` exists.
   Return zero when there are only matches and skipped unsupported cases.

## Result Buckets

Each case ends in exactly one bucket:

| Bucket | Meaning | Exit behavior |
|---|---|---|
| `supported_match` | Candidate ran and normalized issues match the official oracle. | pass |
| `supported_mismatch` | Candidate ran but normalized issues differ. | fail |
| `skipped_unsupported` | Candidate report contains skipped output. | pass |
| `harness_error` | Input mapping, missing report, parsing, or output writing problem. | fail |

Skipped and wrong must never be collapsed into a single failure rate.

Primary metrics:

```text
supported_accuracy = supported_match / (supported_match + supported_mismatch)
coverage = (supported_match + supported_mismatch) / total_cases
```

Coverage is a roadmap metric. Supported accuracy is a correctness metric.

## Normalization

The comparison must not use message text as a primary key.

Normalize rows into:

```rust
struct IssueKey {
    rule_id: String,
    dataset: String,
    domain: String,
    row: String,
    variables: Vec<String>,
    usubjid: String,
    seq: String,
}
```

Rules:

- Uppercase `rule_id`, `dataset`, `domain`, and variable names.
- Split variables on `|`, `;`, or `,`.
- Deduplicate and sort variables.
- Normalize `""`, `null`, `none`, `nan`, `na`, and `n/a` to empty string.
- Do not normalize `"0"` or `"."` to empty string.
- Missing `row`, `usubjid`, or `seq` stays as empty string and remains part of
  the comparison key.
- Do not implement degraded or intersection matching in Phase 1.
- Do not special-case `positive` or `negative`; both compare issue sets against
  the official oracle.
- Treat `error_count` as a display and sanity field, not a primary key.

Minimum column aliases:

| Field | Accepted column names |
|---|---|
| `rule_id` | `rule_id`, `rule`, `core_id`, `core-id`, `id` |
| `dataset` | `dataset`, `dataset_name`, `domain`, `domain_name` |
| `domain` | `domain`, `domain_name` |
| `row` | `row`, `row_number`, `record`, `record_number`, `line`, `line_number` |
| `variables` | `variables`, `variable`, `variable_name`, `column`, `columns` |
| `usubjid` | `usubjid`, `subject`, `subject_id` |
| `seq` | `seq`, `sequence`, `sequence_number` |

## Upstream Lock

Add `tests/open_rules/upstream.lock`.

The lock records the intended upstream repository and pinned SHA. Phase 1 reads
the SHA from the local checkout using git metadata and includes both expected
and observed values in `scoreboard.json`. A mismatch is a warning in Phase 1.

Strict lock enforcement should be added later with CI and baseline policy.

## Outputs

`scoreboard.json` should include:

- Upstream repo, expected SHA, observed SHA, and warnings.
- Summary counts and metrics.
- `by_scope` and `by_case_kind` summaries.
- Full case detail.
- Full `missing` and `extra` issue keys for mismatches.

`summary.md` should include:

- Summary metrics.
- Warnings.
- Supported mismatches first.
- Harness errors second.
- Skipped unsupported counts and a small sample, not exhaustive detail.

JSON is the audit artifact. Markdown is the review artifact.

## Testing

Tests must not require a real `cdisc-open-rules` checkout.

Add synthetic fixtures under `tests/fixtures/open_rules_minimal` with a small
`Published` tree, including `rule.yml`, `.env`, `_datasets.csv`,
`_variables.csv`, dataset CSVs, and official `results.csv`.

Add mirrored candidate report fixtures for match, mismatch, skipped, and missing
candidate cases.

Required test coverage:

- Discovery finds `Published` by default.
- Optional scopes can be included.
- Discovery records `.env`, `_datasets.csv`, `_variables.csv`, dataset file
  paths, and official result paths.
- Normalizer ignores `message`.
- Normalizer handles variable ordering, casing, and null-like values.
- Missing official report produces `harness_error`.
- Missing candidate report produces `harness_error`.
- Candidate skipped row produces `skipped_unsupported`.
- Matching issue sets produce `supported_match`.
- Missing or extra issue keys produce `supported_mismatch`.
- JSON and Markdown outputs are written.
- Non-zero exit decision is returned for mismatch or harness error.

## Documentation And Guardrails

Add `docs/open-rules-oracle-harness.md` with:

- Phase 1 purpose.
- Command examples.
- Input directory structure.
- Candidate report mirror layout.
- Bucket definitions.
- `supported_accuracy` and `coverage` formulas.
- Why message text is not compared.
- Phase 2 and later roadmap.

Add root `AGENTS.md` with concise guidance:

- Treat `cdisc-open-rules` as an oracle-backed compatibility corpus.
- Do not mix skipped and wrong.
- Do not use messages as primary comparison keys.
- Do not change engine semantics in the Phase 1 harness PR.
- Keep `_variables.csv` type authority work for Phase 2.
- Treat LLM-generated data as a second layer after official oracle scoring.

## Follow-On Phases

Phase 2: Add `_variables.csv` schema-aware CSV loading through a dedicated
open-rules data path. Preserve existing generic CSV behavior.

Phase 3: Add harness execution of core-rust cases into the mirrored candidate
report layout.

Phase 4: Add baseline policy, strict upstream lock enforcement, and CI.

Phase 5: Use mismatch clusters to drive targeted engine fixes.

Phase 6: Add LLM-generated augmentation only after official CORE output confirms
expected results.
