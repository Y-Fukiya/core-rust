# Domain Presence reporting

## CORE-000042 output difference

CORE-000042 declares `Rule Type: Domain Presence Check`, `Sensitivity: Dataset`,
and an `exists` condition on TT. Its output variable is TT. The rule checks the
study's dataset inventory, not a physical TT record.

At rules snapshot `1881cb9a1f9a38b6cafc785eca5169b240981f66`, both engines find
one violation in negative/01. Before the correction, core-rust reports dataset
TT, domain TT, and row 1. A fresh official CORE v1.0.0 run reports:

```csv
Dataset,Record,Variable,Value
STUDY,,TT,tt.csv
```

The official JSON issue also has dataset STUDY, an empty row, USUBJID, and SEQ,
and variables `["TT"]`. Its positive/01 execution is **SKIPPED**, with no
issues. The rule's TT scope is absent there. Zero issues do not establish that
the official engine evaluated the positive condition successfully.
core-rust reports Passed with no findings for that fixture, before and after
this correction. The issue comparison matches, but execution-status parity
does not. Applicability and skip-status alignment remain a separate follow-up.

## Cause and correction boundary

The API uses a physical dataset's first row as the evaluation carrier for
Domain Presence. `core_engine::validate_rule` copied that carrier's identity
into the result. It also routed Dataset-sensitive Domain Presence through
`record_level_issues`, introducing a record locator.

Official v1.0.0 at `f3282a0d870bbdf57e326e9992cceea3ed0d9af5` explicitly assigns
STUDY and an empty record locator to Domain Presence output in
`cdisc_rules_engine/models/actions.py`, lines 59-63. Its internal domain marker
is N/A. The official JSON and CSV issue formats do not expose a domain field.

The correction belongs to engine reporting. Dataset sensitivity uses the
existing dataset-level issue constructor. Domain Presence results and issues
use STUDY with no applicable domain or physical record locators. Rust's
existing optional domain field represents this as JSON null and an empty CSV
cell. No report schema change is needed.

Predicates, API evaluation data, operations, scope selection, and skip handling
are unchanged. This is not a new synthetic-study-data implementation. Record
Data rules retain their dataset identity. No rule-ID exception or oracle-driven
scoring relaxation is introduced.

## Runtime provenance

The fresh executions on 2026-11-17 use the official v1.0.0 Apple Silicon release
with each fixture's `.env`, isolated HOME and temporary directories,
`CACHE_TYPE=in_memory`, and the bundled cache. For CORE-000042, the fixture
selects SDTMIG 3.4.

- Rules SHA: `1881cb9a1f9a38b6cafc785eca5169b240981f66`.
- Engine source SHA: `f3282a0d870bbdf57e326e9992cceea3ed0d9af5`.
- Executable SHA-256: `fd0fc1b20122b6adbbab0ba6b0e6c4c959415943b9982d0f4c6571b0fcfea16c`.
- Bundled cache: 222 files, tree SHA-256
  `9b0d65304dced1746f32d5b3f5a7225519925746a9405db3ef3fd10a6b748c8d`.

Each original input file was copied from the pinned checkout and verified
against its tracked content hash. The checkout and cache were unchanged after
execution. This rules snapshot and engine release are a diagnostic pair, not
the snapshot's original engine gitlink.

Local evidence is in `target/core-042-domain-presence-audit/`, including the
probe script, process arguments, fixture hashes, version output, stdout,
stderr, and fresh JSON and CSV reports. These ignored files are not committed
evidence. Archive them before cleaning target. The macOS run does not replace
the Linux-pinned adoption audit.

Fresh executions also covered negative/01 and positive/01 for CORE-000043,
CORE-000044, CORE-000183, CORE-000291, and CORE-000590. Their evidence is in
`target/core-042-domain-presence-family-audit/`. All 12 CSVs, including the
original CORE-000042 pair, equal their stored oracle after CSV parsing.

| Rule | Negative official CSV rows | Positive official status | Positive CSV rows |
| --- | ---: | --- | ---: |
| CORE-000042 | 1 | SKIPPED | 0 |
| CORE-000043 | 1 | SKIPPED | 0 |
| CORE-000044 | 1 | SKIPPED | 0 |
| CORE-000183 | 2 | SUCCESS | 0 |
| CORE-000291 | 1 | SUCCESS | 0 |
| CORE-000590 | 1 | SUCCESS | 0 |

CORE-000183 has two output variables in one study-level finding. Official CSV
counts variable rows, so two CSV rows are not two independent findings.

## Regression verification

The new tests failed before the production change. The engine returned two
findings instead of one and reported AE instead of STUDY. API tests exposed TT,
PC, and AE identities where STUDY was expected. After the change, the eight
Dataset-sensitivity engine tests and nine domain-metadata API tests pass.
They cover generic rule IDs, both compatibility modes, operation-derived
variables, absent targets, input metadata preservation, RecordData controls,
and JSON and CSV readback.

These local workspace checks passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
```

The full candidate snapshot executed 2,298 cases without harness errors. The
full accepted snapshot executed 2,296 cases without harness errors. These are
execution counts, not oracle-conformance passes. The before column below is the
preceding Dataset-sensitivity correction, not the original accepted baseline.

| Full candidate default score | Before | After |
| --- | ---: | ---: |
| Supported match | 2,104 | 2,109 |
| Supported mismatch | 59 | 53 |
| Deferred mismatch | 20 | 21 |
| Deferred skipped | 48 | 48 |
| Unsupported skipped | 55 | 55 |
| No official oracle | 12 | 12 |
| Harness error | 0 | 0 |

The six negative output-format mismatches above now match in strict scoring,
without scoring normalizations. CORE-000677's three mismatches and
CORE-000581's four unsupported skips are unchanged. Full candidate strict
scoring changes from 1,270 matches and 927 mismatches to 1,275 matches and
922 mismatches. Its 89 unsupported skips and 12 missing-oracle cases remain.

There is also a compatibility cost. Candidate CORE-000560 negative/01 changes
from supported match to deferred mismatch under the existing default policy.
The next section explains the fresh diagnostic. No new deferral or scorer
policy was added to suppress this difference.

Against the accepted snapshot, five negative cases lose default matches:
CORE-000042, 043, 044, 183, and 560. Supported mismatches increase from 13 to 17,
and deferred mismatches from 6 to 7. Its stored outputs use the older physical
dataset identity. The accepted full regression gate is **not green**.

The local evidence directory also contains `candidate-scoreboard.json`,
`candidate-strict-scoreboard.json`, `accepted-scoreboard.json`, and workspace
check logs. No GitHub CI run for this uncommitted change is claimed.

## CORE-000560 follow-up

The candidate fixture's stored CSV reports ADVS and a `results` variable.
Existing rule-specific scorer normalization rewrites the variables to ADSL
but keeps ADVS identity. The corrected engine reports STUDY with ADSL, so the
stored comparison now disagrees. Do not reintroduce physical dataset identity
to preserve that historical match.

Its `.env` contains `PRODUCT=ADAMIG` and an empty `VERSION`. The unchanged
fixture fails official v1.0 CLI argument validation with exit 2 and
`Missing option '-v' / '--version'`. There is no validation report from that
attempt. An initial strict-profile probe rejected the same missing version
before launching validation.

A separate diagnostic explicitly selects `-s adamig -v 1-3`, a version named
in the rule's authority. It leaves the input files and `.env` unchanged and
records the override. Official v1.0 then reports:

```csv
Dataset,Record,Variable,Value
STUDY,,ADSL,Not in dataset
```

The negative status is ISSUE REPORTED with one variable row. The positive
status is SUCCESS with no findings. This supports the study-level reporting
contract, but the explicit version override is **not** an equivalent-condition
reproduction of the original fixture or authorization to replace its oracle.
Evidence is in `target/core-042-domain-presence-560-audit-v2/`.

Before adopting a new oracle for CORE-000560, select and record its intended
ADaMIG version, regenerate with the pinned official engine, and review the
existing rule-specific normalization independently of this engine change.
That fixture and scoring-policy task remains open.

## Adoption boundary

The accepted rules pin remains `7f7fae49376b3d023563ebb6c36a3b392d6e649f`.
Stored upstream oracles, accepted baselines, manifests, and scoring policy are
unchanged. This correction does not resolve the other mismatch families or
authorize adopting the candidate snapshot. Merge remains on hold.
