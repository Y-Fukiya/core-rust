# Open Rules Upstream Status

Current full upstream compatibility is tracked by the scheduled Open Rules
Upstream workflow.

The repository-local Open Rules fixture in CI is the enforced PR gate. That
fixture requires full coverage, zero skipped unsupported cases, and a clean
baseline comparison.

The full upstream workflow is an observe job. It pins `cdisc-open-rules` with
`tests/open_rules/upstream.lock`, runs the scorer, and uploads
`scoreboard.json` / `summary.md` artifacts for review. Full upstream is not
expected to be 100% covered unless the generated scoreboard says so for that
specific run.

Use the uploaded scoreboard artifacts to inspect the current
`supported_match`, `supported_mismatch`, `skipped_unsupported`,
`no_official_oracle`, and `harness_error` buckets. Do not treat historical run
logs as current conformance claims.
