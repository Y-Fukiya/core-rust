# Dataset sensitivity reporting

## CORE-000013 cause

CORE-000013 declares `Rule Type: Record Data`, `Sensitivity: Dataset`, and an
`exists` condition on `AESTAT`. A present column violates the rule regardless of
how many records have a value. The dataset, not each record, is the reporting
unit.

Before this change, `core_engine::validate_rule` routed Dataset-sensitive Record
Data rules to `record_level_issues`. The predicate correctly found the forbidden
column, but the reporting branch emitted 13 findings with record locators for
the 13-row negative fixtures.

The existing `Sensitivity` enum and `dataset_level_issues` function already
represent the required behavior. Record Data rules with Dataset sensitivity
must use that function. Condition evaluation remains unchanged. The result
retains the dataset, domain, rule ID, and output variables, but has no record,
USUBJID, or sequence locator. Record sensitivity still reports individual
matching records. Domain Presence and metadata reporting are separate paths.

CORE-000165 also had an API postprocessor that split a missing output variable
into an extra Record 0 issue. That postprocessor now applies only to Record
sensitivity. Dataset findings retain both output variables in one issue with
no record locator. The updated regression first reproduced two issues where
one was expected, then passed after the guard changed.

## Oracle evidence and adoption boundary

The diagnostic rules snapshot is
`1881cb9a1f9a38b6cafc785eca5169b240981f66`. Its CORE-000013 negative/01 and
negative/02 fixtures each expect one finding, with an empty Record field. The
positive/01 fixture expects none.

The official CDISC v1.0.0 engine at
`f3282a0d870bbdf57e326e9992cceea3ed0d9af5` also reports one finding for each
negative fixture in [diagnostic run 43900071480](https://github.com/Y-Fukiya/core-rust/actions/runs/43900071480).
Its [Dataset action](https://github.com/cdisc-org/cdisc-rules-engine/blob/f3282a0d870bbdf57e326e9992cceea3ed0d9af5/cdisc_rules_engine/models/actions.py#L139)
creates one error object without a record index. The rules snapshot and engine
tag are a diagnostic pair, not the original packaged combination.

The accepted rules pin remains
`7f7fae49376b3d023563ebb6c36a3b392d6e649f`. Its CORE-000013 rule also declares
Dataset sensitivity, but its stored negative oracle lists individual records.
Correcting the reporting behavior therefore changes comparisons with that old
oracle. Do not hide this difference through scorer normalization or a rule-ID
exception. Adoption of a newer oracle and review of affected baselines remain
separate from this engine correction.

## Remaining Dataset case

CORE-000457 positive/01 is not an output-multiplicity problem. Its stored oracle
has two variable rows for SUPPEC, while core-rust reports no findings. The fixture
lists `ec` in `_datasets.csv`, and the reported `RDOMAIN` value is `EC`. A fresh
official v1.0.0 run confirmed that the malformed EC header and embedded metadata
prevent official `study_domains` from recognizing EC. Restoring the DOMAIN
header and removing those metadata rows makes official CORE report no findings.
See the [fixture investigation](open-rules-core-000457-fixture-gap.md) for the
controlled runs and upstream correction request. The pinned fixture, stored
oracle, baselines, and original mismatch remain unchanged.

## Local verification results

The final implementation was rerun on macOS against both unchanged snapshots.
Candidate execution completed 2,298 of 2,298 cases without a harness error.
Accepted-snapshot execution completed 2,296 of 2,296 cases without a harness
error. These local runs do not replace the Linux CI gate.

| Candidate default score | Before | After |
| --- | ---: | ---: |
| Supported match | 2,070 | 2,104 |
| Supported mismatch | 84 | 59 |
| Deferred mismatch | 25 | 20 |
| Deferred skipped | 52 | 48 |
| Unsupported skipped | 55 | 55 |
| No official oracle | 12 | 12 |

Of the 31 previously mismatching Dataset-family cases, 30 now match and
CORE-000457 positive/01 remains a mismatch. The same 30 cases match in strict
scoring without oracle-informed alignment. CORE-000013's two negatives each
produce one finding, and its positive produces none in both modes.
No candidate case previously marked
supported match became a mismatch or skip. Four additional deferred skips became
supported matches; they are outside the 31-case mismatch denominator.

Full candidate strict scoring changes from 1,232 matches and 965 mismatches to
1,270 matches and 927 mismatches. Its 89 unsupported skips and 12 missing-oracle
cases are unchanged. Strict scoring is still not a corpus-wide pass.

The unchanged accepted snapshot now has 13 supported mismatches and six deferred
mismatches where it previously matched. Its affected rule IDs are CORE-000013,
015, 030, 084, 096, 098, 118, 165, 166, 167, 171, 180, 186, and 261. These
19 differences remain visible. The accepted regression gate is **not green**
with this change, and its full upstream baseline and lock have not been updated.

Workspace formatting, `cargo check --workspace --locked`,
`cargo clippy --workspace --locked -- -D warnings`, and
`cargo test --workspace --locked` passed locally. Failing-before regressions
cover CORE-000013 multiplicity and CORE-000165's extra row-zero issue. The
existing local golden fixture was corrected to expect no Dataset row locator;
the full upstream oracle baseline was not edited. The subsequent curated
checkpoint review is recorded below.

## CORE-000108 curated checkpoint review

After the P21PORT smoke output was moved outside the Cargo cache,
[CI run 43930387550](https://github.com/Y-Fukiya/core-rust/actions/runs/43930387550)
reached the curated gap gate and reported one fingerprint regression for
`Published/CORE-000108/negative/02`.

The accepted rule declares `Rule Type: Record Data` and `Sensitivity: Dataset`.
The Dataset reporting correction removes the old record locator from the DM
finding. Before the correction, the CSV reported row 2, USUBJID `CDISC002`, and
sequence 1. The corrected CSV leaves those record locators empty. The finding,
dataset, and output variables `DTHFL` and `USUBJID` remain present.

The scorer represents that finding as two structural variable keys. Their only
changed field is `row`, from `2` to empty. Existing identity alignment already
leaves USUBJID and sequence empty in both normalized outputs. No new scoring
normalization is introduced. The candidate fingerprint changes from `28d6cf25ded95ca5` to
`6529f95af3fb7ded`.

Only that fingerprint in `curated-gap-baseline.json` is updated. The official
issue count remains 0, the candidate and extra counts remain 2, and the bucket
remains `deferred_oracle_gap_skipped`. This records the reviewed candidate output
format; it does not resolve the empty official oracle or promote the case to a
supported match. The full upstream baseline, accepted pin, upstream fixtures,
and scoring policy remain unchanged.

A fresh run of the 31-rule curated gap subset reproduced one regression before
this checkpoint update and zero afterwards. The two improvements for
CORE-000698 and CORE-000704 remain reported. This narrow checkpoint update does
not accept the 19 full-corpus comparison differences described above.

## Diagnostic inventory follow-up

The separate candidate-triage CSV currently writes the rule's YAML Scope object
into its `scope` column, rather than the corpus scope `Published`. This audit
uses the explicit Published corpus and unique `(rule_id, case_kind, case_id)`
keys for its 31-case family. Restore the corpus-scope field in a separate
harness change before using that CSV as a multi-scope case identity source.
