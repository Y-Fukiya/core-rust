# Release Reproducibility

`core-rust` is still a technical preview, but release artifacts should be
reviewable and reproducible enough for internal audit use. Do not present these
artifacts as regulatory submission evidence unless a separate governed release
process has approved them.

## Provenance Manifest

Before publishing a binary, archive, or validation-harness artifact, write a
release provenance manifest:

```sh
cargo run -p xtask -- release-manifest \
  --out target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --target-triple <expected-target-triple> \
  --artifact target/release-provenance/<release-artifact>
cargo run -p xtask -- release-verify \
  --manifest target/release-provenance/release-manifest.json \
  --artifact-root target/release-provenance \
  --source-root . \
  --target-triple <expected-target-triple> \
  --require-clean-git \
  --require-ci-run-url \
  --require-source-date-epoch
```

The manifest records:

- manifest schema version
- xtask package name and version
- git commit and dirty-worktree status
- whether git provenance was available when the manifest was written
- optional artifact paths and SHA-256 digests for files passed with `--artifact`
- `Cargo.lock` SHA-256 when available
- Rust target triple from `--target-triple`, or from `rustc -vV` when the
  option is omitted
- GitHub Actions run URL when written inside GitHub Actions
- GitHub Actions run metadata when available: run id, run attempt, workflow,
  job, ref name/type, commit SHA, and actor
- optional `SOURCE_DATE_EPOCH`
- verification commands expected for release review

When `--artifact-root` is supplied, artifact paths are recorded relative to that
root so long-lived manifests do not leak local absolute paths. Artifacts outside
the root are rejected.
`release-verify` recomputes artifact SHA-256 values and returns a failing exit
status when a recorded artifact is missing or has changed. When the manifest
contains a `Cargo.lock` SHA-256, `release-verify` also checks `Cargo.lock` under
`--source-root` so dependency drift is caught before archive publication. If
`--source-root` is omitted, the current working directory is used.
The main CI workflow includes a host release artifact provenance gate that
builds the `core-rs` release binary, records its SHA-256 in
`release-manifest.json`, and verifies the manifest against the built artifact.
It also includes a lightweight multi-target release provenance smoke gate that
creates and verifies multi-artifact manifests for more than one target triple.
Use the stricter policy flags for reviewed release bundles:

- `release-manifest --target-triple <triple>` records the reviewed build target
  explicitly; otherwise the host triple from `rustc -vV` is recorded.
- `release-verify --target-triple <triple>` requires the manifest's recorded
  Rust target triple to match the reviewed build target.
- `--source-root <dir>` points verification at the source checkout whose
  `Cargo.lock` should match the manifest.
- `--require-clean-git` requires available git provenance and `dirty=false`.
- `--require-ci-run-url` requires the manifest to record the GitHub Actions run
  URL that produced or verified the artifact.
- `--require-source-date-epoch` requires a recorded `SOURCE_DATE_EPOCH`.

If `dirty` is `true`, do not publish the artifact as a reviewed release unless
the uncommitted diff is intentionally included and separately archived.
If git provenance is unavailable, do not treat the manifest as evidence of a
clean source checkout.

## Verification Gate

Run these commands before tagging or publishing:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
PYTHONPATH=src python3 -m pytest -q
```

For XPT parser robustness review, run the fuzz target manually as described in
[`docs/xpt-fuzzing.md`](xpt-fuzzing.md). The fuzz target is intentionally an
audit/robustness tool rather than a default release gate.

For P21PORT conversion artifacts, also run a representative read-only workflow
against the committed fixture corpus:

```sh
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir <p21-workflow-out>
```

This P21PORT smoke check exercises `build-readonly`, `generate`,
`validate-structure`, real `run-core` orchestration through a reviewed fake
engine, `compare-results` against committed golden fixtures, an expected
comparison failure for equal-count structural mismatch and extra-issue
directions, fuzzy mapping, duplicate P21 rule-id preservation, and unsupported
generation probes. It is not a Pinnacle 21 or official CDISC Validator
equivalence check.

The default pytest configuration excludes subprocess-heavy integration tests.
Run the P21PORT smoke explicitly with either:

```sh
PYTHONPATH=src python3 -m pytest -q -m integration
PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir target/p21port-smoke
```

For Open Rules compatibility artifacts, also archive the default scoreboard,
strict scoreboard, and default-vs-strict delta artifact from the upstream
workflow. The delta is the first place to inspect how much compatibility scoring
changes the headline metrics.

## Reproducibility Notes

- Prefer `--locked` Cargo commands so dependency versions come from
  `Cargo.lock`.
- Set `SOURCE_DATE_EPOCH` when producing reproducible release bundles.
- Keep generated Open Rules scoreboards canonicalized before committing them as
  baselines.
- Store release artifacts together with `release-manifest.json`, the exact git
  commit, and CI run URLs.
- Pass every reviewed binary/archive through `--artifact` and, where practical,
  pair it with `--artifact-root` so its SHA-256 digest and portable relative
  path are recorded in the manifest.
- For multi-platform or multi-format releases, write one manifest per target
  triple and pass every reviewed binary, archive, and generated harness bundle
  for that target as separate `--artifact` values. The verifier checks each
  recorded artifact independently, so a single changed target in a release
  matrix fails verification.
- Treat the CI host release artifact gate as the actual binary provenance check.
  The separate multi-target release smoke is manifest/verification plumbing
  coverage: it checks that target-triple policy and multiple artifact hashes are
  represented and verified, but it does not replace actual cross-target builds.
- Treat `supported_accuracy = 100%` as a regression-gate invariant over the
  supported denominator, not as a claim of full regulatory conformance.
