# XPT Fuzzing

The XPT parser has deterministic boundary tests in `core-data` and a
`cargo-fuzz` target for malformed byte streams.

## Target

```bash
cargo install cargo-fuzz
cd fuzz
cargo fuzz run xpt_parser
```

The target writes each fuzz input to a temporary `.xpt` file and calls
`core_data::load_xpt_dataset`. Inputs larger than 2 MiB are ignored so fuzzing
stays focused on parser correctness rather than memory pressure.

## Scope

This is a robustness harness. It checks for panics, unchecked arithmetic, and
resource handling problems in malformed XPT input. It is not a semantic
conformance check for SAS XPORT content and it is not part of the default
workspace test gate.

The repository also includes a short manual/scheduled GitHub Actions workflow
(`XPT Fuzz`) that runs the target with `-max_total_time=60`. Treat it as a
periodic robustness audit artifact rather than a release-blocking CI gate.
The workflow uses the committed seed corpus under `fuzz/corpus/xpt_parser` and
uploads minimized failure artifacts from `fuzz/artifacts` when fuzzing fails.
The seed corpus intentionally stays small and reviewable: it includes malformed
library/header bytes plus NAMESTR-header, observation-padding, and numeric
payload entry points. Add minimized crash reproducers or new reviewed boundary
seeds to this directory rather than committing the generated fuzz corpus.

Use the deterministic `core-data` XPT tests for regression coverage of known
boundaries such as NAMESTR length, IBM floating-point decoding, observation
padding, invalid numeric payloads, and row/cell size caps.
