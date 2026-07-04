# XPT Fuzzing

The XPT parser has deterministic boundary tests in `core-data` and a
`cargo-fuzz` target for malformed byte streams.

## Target

```bash
cargo install cargo-fuzz
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

Use the deterministic `core-data` XPT tests for regression coverage of known
boundaries such as NAMESTR length, IBM floating-point decoding, observation
padding, invalid numeric payloads, and row/cell size caps.
