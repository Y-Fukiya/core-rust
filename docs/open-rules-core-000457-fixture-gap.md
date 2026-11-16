# CORE-000457 positive fixture gap

The remaining Dataset-family mismatch is caused by how the two loaders read
the malformed positive EC fixture. It is not evidence that core-rust should
report a missing EC parent. No engine, scorer, manifest, baseline, or accepted
rules pin changes are part of this investigation.

## Rule and fixture

The rule at rules snapshot `1881cb9a1f9a38b6cafc785eca5169b240981f66`
checks whether a supplemental dataset's `RDOMAIN` belongs to `$study_domains`.
It declares Dataset sensitivity and outputs `RDOMAIN` and `USUBJID`.

`Published/CORE-000457/positive/01/data/_datasets.csv` lists EC. SUPPEC contains
`RDOMAIN=EC`, but `ec.csv` has 17 empty header cells followed by three rows of
column labels, types, and lengths. Actual EC records start after those rows.
`_variables.csv` has no EC variable definitions.

The stored oracle has two CSV rows: `SUPPEC/RDOMAIN/EC` and
`SUPPEC/USUBJID/CDISC009`, both without Record values. These are two output
variable rows, not two independent dataset findings.

## Cause

In official engine v1.0.0 at `f3282a0d870bbdf57e326e9992cceea3ed0d9af5`:

- `services/csv_metadata_reader.py` reads the first CSV record without removing
  the metadata preamble.
- `models/sdtm_dataset_metadata.py` derives the domain from that record's
  `DOMAIN` field. The blank header makes that field absent.
- `operations/study_domains.py` collects those dataset domains. EC is missing,
  so the rule reports SUPPEC.

core-rust's Open Rules data-dir loader restores common column names from labels,
removes the metadata preamble, and uses the dataset manifest name as its domain.
`derive_study_domains_dataset` includes that domain and dataset name, so EC is
present. That repair predates the Dataset sensitivity change. Reporting a
missing EC parent here would contradict the loaded dataset inventory.

## Controlled execution

Fresh executions on 2026-11-17 used the official v1.0.0 Apple Silicon release
with SDTMIG 3.2 from each fixture's `.env`. The environment was isolated, with
`CACHE_TYPE=in_memory` and the bundled cache. This is a macOS diagnostic, not a
replacement for the Linux-pinned audit or evidence of full-corpus conformance.

| Input copy | Official status | Official CSV rows | core-rust scored rows |
| --- | --- | ---: | ---: |
| Original negative/01 | ISSUE REPORTED | 2 | 2 |
| Original positive/01 | ISSUE REPORTED | 2 | 0 |
| Positive: name only the second header cell DOMAIN | ISSUE REPORTED | 2 | Not run |
| Positive: also remove the label, type, and length rows | SUCCESS | 0 | 0 |
| Positive: remove EC and its dataset manifest entry | ISSUE REPORTED | 2 | 2 |

All five official processes exited zero and produced a valid single-rule
`Rules_Report`. The original positive CSV equals the stored oracle structurally.
The two modified copies tested with core-rust match their fresh official
results in strict scoring, with no scoring normalizations. The original
positive mismatch remains visible.

The header-only control still has `Domain Abbreviation` as its first DOMAIN
value. Removing the three metadata rows exposes `EC`. This separates the
metadata-reading cause from output multiplicity and rule applicability.

The diagnostic transformation on a copy is exactly:

```python
rows = list(csv.reader(stream))
assert rows[0] == [""] * 17
assert rows[1][1] == "Domain Abbreviation"
assert rows[2][1] == "Char" and rows[3][1] == "50"
rows[0][1] = "DOMAIN"
rows = [rows[0], *rows[4:]]
```

This probe names only the field needed to test the cause. It is not a complete
upstream fixture repair: the other blank headers still need correct names.

Runtime provenance:

- Release asset: `core-mac-apple-silicon.zip`, tag `v1.0.0`.
- Archive SHA-256: `8c5b274d24def4bffb983f2a835121f79854a0028f605a49e336fdaca5d87361`,
  checked against the release asset digest.
- Executable SHA-256: `fd0fc1b20122b6adbbab0ba6b0e6c4c959415943b9982d0f4c6571b0fcfea16c`.
- Cache: 222 files, tree SHA-256
  `9b0d65304dced1746f32d5b3f5a7225519925746a9405db3ef3fd10a6b748c8d`,
  equal to the pinned Linux audit cache.

Local evidence is in `target/core-457-fixture-audit-v2/`: `summary.json`,
`probe.py`, per-case original and effective input hashes, process arguments,
stdout, stderr, and fresh JSON and CSV reports. `candidate-probe-score/` contains
the strict comparison of the repaired-header and missing-parent copies. These
ignored local artifacts are not committed evidence and must be archived before
cleaning `target`. The earlier `core-457-fixture-audit/` attempt failed before
validation because its temporary Unix socket path was too long; it is not
counted as a validation run.

## Upstream correction request

The required upstream change is to give all EC columns their correct names,
remove the label/type/length preamble, and supply matching EC entries in
`_variables.csv`. Regenerate the positive oracle with the pinned official
engine after reviewing those column mappings. Keep a negative control with EC
absent. The minimal DOMAIN-only probe is evidence for this request, not the
fixture to adopt.

This request is not submitted or adopted by this change. Do not clear the
stored mismatch, add an oracle-gap exclusion, or change the accepted baseline
merely because the diagnostic copy passes. Adoption still needs an upstream
fixture decision and the full regression run.

## Local regression protection

`run_validation_core_000457_checks_supplemental_parent_presence` covers a normal
EC CSV, an embedded-metadata EC CSV without EC schema entries, and an absent EC
dataset. It checks recovered DOMAIN values and row counts, pass/fail status,
and Dataset-level output variables without record locators. Expected behavior
comes from parent dataset presence, not from copying the stored positive oracle.

```sh
cargo test -p core-api --locked --lib run_validation_core_000457_checks_supplemental_parent_presence
```
