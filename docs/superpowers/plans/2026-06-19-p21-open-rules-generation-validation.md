# P21 Open Rules Generation And Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Extend the read-only P21 Open Rules toolkit with conservative generated-rule output and structural validation, moving the overall conversion toolkit toward the 80% implementation mark.

**Architecture:** Keep the read-only pipeline unchanged and add optional generation commands. `generate` reads classified canonical rules, emits `output/generated_rules/<id>/` folders for `AUTO_CONVERTIBLE` rules only, and `validate-structure` verifies generated folders without running the Rust engine or writing into Open Rules `Published`.

**Tech Stack:** Python 3.11+, standard library, PyYAML, pytest.

---

## Scope

In scope:

- Add `GeneratedRuleManifest` model.
- Emit `conversion_status.jsonl` so generation has full canonical fields.
- Generate conservative CORE-style `rule.yml` for simple `Regex`, `Match`, and same-record `Condition` candidates when enough source data exists.
- Generate deterministic minimal positive and negative CSV test data.
- Generate `.env`, `_datasets.csv`, `_variables.csv`, `expected_results.csv`, and `manifest.json`.
- Add structural validation for generated folders.
- Add CLI commands `generate` and `validate-structure`.

Out of scope:

- Running official CORE.
- Writing into `input/cdisc-open-rules/Published`.
- Exporting generated rules to `Unpublished`.
- Full cross-domain, CT, Define.xml, Schematron, SUPP--, RELREC, or external lookup conversion.

## Tasks

- [x] Add failing tests for generated Regex rule folders.
- [x] Add failing tests for structural validation.
- [x] Add failing CLI test for `build-readonly` -> `generate` -> `validate-structure`.
- [x] Implement `GeneratedRuleManifest`.
- [x] Implement `convert_core.py` and `generate_testdata.py`.
- [x] Implement `validate_outputs.py`.
- [x] Extend `emit_conversion_status` to write JSONL.
- [x] Extend `cli.py` with `generate` and `validate-structure`.
- [x] Run `python -m pytest tests/cdisc_rulekit -q`.
- [x] Commit only generation/validation toolkit files.
