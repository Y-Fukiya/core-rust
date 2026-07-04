# Release Reproducibility

`core-rust` is still a technical preview, but release artifacts should be
reviewable and reproducible enough for internal audit use. Do not present these
artifacts as regulatory submission evidence unless a separate governed release
process has approved them.

## Provenance Manifest

Before publishing a binary, archive, or validation-harness artifact, write a
release provenance manifest:

```sh
cargo run -p xtask -- release-manifest --out target/release-provenance/release-manifest.json
```

The manifest records:

- manifest schema version
- xtask package name and version
- git commit and dirty-worktree status
- optional `SOURCE_DATE_EPOCH`
- verification commands expected for release review

If `dirty` is `true`, do not publish the artifact as a reviewed release unless
the uncommitted diff is intentionally included and separately archived.

## Verification Gate

Run these commands before tagging or publishing:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
PYTHONPATH=src python3 -m pytest -q
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
- Treat `supported_accuracy = 100%` as a regression-gate invariant over the
  supported denominator, not as a claim of full regulatory conformance.
