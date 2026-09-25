# Rule Validation Pilot

Use this checklist for a bounded Open Rules / P21PORT pilot, not a claim of
Pinnacle 21 equivalence or approval for submission decisions.

## Select And Review The Rules

1. Record the core-rust commit and the verified engine binary/release manifest.
2. Pin the Open Rules commit and the standard/version. Keep proposed upstream
   refreshes separate from the accepted baseline; unresolved differences must
   remain visible.
3. Select 10-20 permitted rules representative of the intended use. Include
   required/missing values, scalar and pattern checks, and, when applicable,
   cross-dataset relationships. Do not fill the set with unsupported semantics
   merely to reach a numerical target.
4. For P21PORT, retain the permitted source catalog, extraction report, ID
   mapping/confidence, and generated draft. A mapped ID does not establish
   semantic equivalence. Review the generated conditions before execution.
5. Have a reviewer derive normal, violating, and boundary cases from the rule
   text independently of the generated test expectations. Record source IDs,
   expected issue locations/variables, reasoning, reviewer, and review date.
   The included synthetic smoke fixtures do not satisfy this independent-review
   requirement. No licensed real P21 catalog is supplied by this checklist.

## Execute In A New Directory

### Reviewed Open Rules Fixture Pilot

`tests/open_rules/pilot-review.json` freezes 12 rules / 24 official fixtures at
the accepted upstream pin. Expectations (40 negative issue rows, zero positive
issue rows) were cross-checked by Codex against rule text and fixture data, not
copied from candidate reports. Each entry records its reasoning and a hash of
the entire official rule directory, including hidden metadata and results.
The set covers conditional requiredness, missing values, literal comparisons,
domain presence, reference distinct, subject joins, and full-date ordering.
It does not cover all operators, partial-date boundaries, or actual P21 data.

CI checks the source hashes and exact official `(Dataset, Record, Variable)`
multisets. It separately compares candidate full identities (rule, dataset,
domain, row, variable, USUBJID, sequence) against frozen expectations. The
`row_context` values were read from the specified input records, not generated
from candidate reports. Duplicate/missing findings and skipped execution fail.

The strict scorer remains a separate, non-gating audit: this pilot has 14 strict
matches and 10 strict mismatches because candidate findings contain subject and
sequence context absent from the official CSV. A strict mismatch is **not**
converted to a strict match; the full scoreboard is retained. The gate requires
full frozen-identity agreement, the exact case set/counts, and no normalization,
deferred, skipped, missing-oracle or harness-error cases. It permits the scorer's
ordinary nonzero mismatch exit only when those independent checks pass.

This is a regression gate for reviewed fixtures, **not independent human
approval**. `human_approval: pending` remains explicit until a qualified reviewer
records a separate approval.

The `open-rules-pilot-evidence` CI artifact contains candidate reports, the
strict scoreboard, and `fixture-review.json`. A source-only check records
`candidate_check: not_performed`; a failed/interrupted run must not be treated
as successful just because the artifact exists. Re-review any source hash
change before updating the manifest; never regenerate expectations from engine
output to make the gate pass.

### P21PORT Draft Execution

After preparing and reviewing the generated rule tree, run from the repository
root with the appropriate pinned standard/version in the engine command:

```sh
python -m cdisc_rulekit.cli run-core \
  --generated-rules output/pilot/generated_rules \
  --out output/pilot/run-001 \
  --engine-command '/absolute/path/to/core-rs validate --standard SDTMIG --standard-version 3.4'

python -m cdisc_rulekit.cli compare-results \
  --generated-rules output/pilot/generated_rules \
  --actual-root output/pilot/run-001/core_runs \
  --out output/pilot/run-001/comparison \
  --strict-structure
```

Choose another run directory for every execution or dry run. Do not add
`--strict` to the engine command just to test deliberately violating fixtures:
negative fixtures are expected to produce findings. Use the separate structural
comparison to check whether those findings are correct. A successful process
exit alone is not a successful validation comparison.

## Read The Evidence

`reports/core_run_evidence.json` records a unique run ID and UTC times, the run
plan, engine working directory, worker/timeout settings, input hashes, invoked
executable hashes, execution results, and report hashes. Data snapshots include
metadata and `.env` files; rule manifests and expected-results files are included
when present. They are not copied into the evidence JSON.

- `started`: execution/finalization did not finish; never treat it as a pass.
- `failed`: a process failed, inputs changed, or evidence was incomplete.
- `completed`: processes completed successfully, reports exist, and the before
  and after input/executable snapshots agree. Report content still needs the
  separate comparison and human review.
- `comparison_status: not_performed` and `approval.status: not_reviewed` are
  intentional. This command neither compares expectations nor signs approval.

The evidence is not an immutable-input sandbox or a signed attestation. A file
changed and restored between snapshots cannot be detected. Hashing a Python,
shell, or Cargo launcher does not identify its scripts, dependencies, downloaded
resources, or resulting engine binary; those require a separate pinned source
and release manifest. Environment variables and external files mentioned in
arbitrary engine options are not fully captured. Use immutable staged inputs
and a verified direct binary for a reviewed run.

## Retain And Approve

Archive the exact input/rule tree (including hidden metadata), source and mapping
records, engine release manifest, run directory, and comparison directory
together. Check that the comparison used the same expected fixtures and report
files whose hashes were recorded. Make a separate approval record referencing
the run ID, evidence-file SHA-256, comparison-file SHA-256, reviewer, date, and
accepted limitations. Do not edit the original evidence to manufacture approval.

Run paths, command arguments, stdout, and stderr can contain local paths or
sensitive values. Review the bundle before external sharing; do not publish
study data automatically. CI uploads only repository-owned synthetic smoke
inputs/outputs, with separate artifact names for the synthetic engine workflow
and the real engine on synthetic fixtures. The presence of an artifact alone
does not mean its job passed; check the job result and evidence states.

Real-data pilot approval and adoption of the proposed upstream refresh remain
separate tasks. This implementation adds traceability, not new rule coverage.
