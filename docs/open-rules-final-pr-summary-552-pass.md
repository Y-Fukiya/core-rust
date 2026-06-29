# P21→OPEN RULES Conversion (SDTM-IG) — PR Readiness Summary

## Scope and target

- Scope: SDTM-IG P21 rule conversion candidates for final expanded pilot.
- Input rule set: `output/sdtmig_full_rerun_20260621_expanded_v3_unique_guard/`
- Official CORE execution input: `output/sdtmig_full_rerun_20260621_expanded_v3_unique_guard_official_core_final_version_template/`
- Command chain used:
  - `python -m cdisc_rulekit.cli build-readonly ...`
  - `python -m cdisc_rulekit.cli generate ... --include-fuzzy-candidates`
  - `python -m cdisc_rulekit.cli validate-structure ...`
  - `python -m cdisc_rulekit.cli run-core ... --data-mode data-dir --output-mode file-base`
  - `python -m cdisc_rulekit.cli compare-results ...`
  - `python -m cdisc_rulekit.cli export-rules ... --only-passed`

## Final measured outcome (run `...final_version_template`)

- Generated rule directories: `552`
- Rule execution rows: `1104`
- Comparison rows passed: `1094`
- Comparison rows non-pass: `10`
- Supported mismatches: `0`
- CORE skipped coverage-gap rows: `10` (`5` rules)
- PASS rules: `547`
- Skipped rules (not exported): `5`

## PR-ready export status

- Export command:
  `python -m cdisc_rulekit.cli export-rules --generated-rules <generated>/generated_rules --open-rules-repo <repo> --comparison-summary <run>/reports/comparison_summary.csv --only-passed --target-subdir Unpublished/NEW-RULE/FINAL-PASS-552`
- Export result: `547` exported, `5` skipped
- Export manifest: `export_manifest.csv` / `export_manifest.json`

### Evidence files to attach

- `output/sdtmig_full_rerun_20260621_expanded_v3_unique_guard_official_core_final_version_template/reports/comparison_summary.csv`
- `output/sdtmig_full_rerun_20260621_expanded_v3_unique_guard_official_core_final_version_template/reports/official_core_failure_classification.md`
- `output/sdtmig_full_rerun_20260621_expanded_v3_unique_guard_official_core_final_version_template/reports/comparison_summary.md`

## Risk statement

- This PR is scoped to PASS rules only.
- The five skipped rules are not false-negatives; they are currently treated as
  official CORE coverage gaps.

