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
  --artifact <release-artifact>
```

The manifest records:

- manifest schema version
- xtask package name and version
- git commit and dirty-worktree status
- whether git provenance was available when the manifest was written
- optional artifact paths and SHA-256 digests for files passed with `--artifact`
- `Cargo.lock` SHA-256 when available
- Rust target triple when `rustc -vV` is available
- GitHub Actions run URL when written inside GitHub Actions
- optional `SOURCE_DATE_EPOCH`
- verification commands expected for release review

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
engine, and `compare-results` against committed golden fixtures. It is not a
Pinnacle 21 or official CDISC Validator equivalence check.

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
- Pass every reviewed binary/archive through `--artifact` so its SHA-256 digest
  is recorded in the manifest.
- Treat `supported_accuracy = 100%` as a regression-gate invariant over the
  supported denominator, not as a claim of full regulatory conformance.
