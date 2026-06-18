# CDISC Open Rules Oracle Harness

`core-rust` treats `cdisc-org/cdisc-open-rules` as an oracle-backed
compatibility corpus. Each case is a combination of `rule.yml`, test data under
`data/`, and committed official `results/results.csv`.

Phase 1 is read-only. It scores existing core-rust `report.csv` files against
official `results.csv` files. It does not run core-rust, alter engine behavior,
apply baselines, or load `_variables.csv` as a schema authority.

## Command

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard
```

The default scope is `Published`. Add `--scope Unpublished` to include another
scope.

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
| `harness_error` | Official or candidate report is missing, malformed, or cannot be scored. | non-zero |

Skipped and wrong are separate. Skipped cases are coverage gaps. Supported
mismatches are correctness problems.

## Metrics

```text
supported_accuracy = supported_match / (supported_match + supported_mismatch)
coverage = (supported_match + supported_mismatch) / total_cases
```

Coverage can be low while supported accuracy is high. That means the roadmap is
large, not that supported behavior is wrong.

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

## Phase Roadmap

Phase 2 adds `_variables.csv` schema-aware CSV loading through a dedicated Open
Rules data path.

Phase 3 runs core-rust against selected cases and writes candidate reports into
the mirrored layout.

Phase 4 adds baseline policy, strict upstream lock enforcement, and CI.
