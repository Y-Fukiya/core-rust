# Python compatibility fixtures

This directory stores Python/CDISC-compatible expected validation outputs.

Each `cases/*.json` file defines one comparison case:

- `rule_paths`: rule files or direct-child rule directories, relative to `tests/fixtures`
- `dataset_paths`: dataset files or direct-child dataset directories, relative to `tests/fixtures`
- `define_xml_paths`: optional Define-XML files, relative to `tests/fixtures`
- `ct_paths`: optional controlled terminology JSON files, relative to `tests/fixtures`
- `include_rules` / `exclude_rules`: optional rule filters
- `standard` / `standard_version`: optional standard filters
- `expected_path`: stored Python/CDISC expected output, relative to `tests/fixtures`

The Rust harness compares only stable validation fields:

- rule_id
- execution_status
- skipped_reason
- dataset
- domain
- message
- error_count
- issue row, variables, and message

It intentionally ignores JSON key order, elapsed time, warning wording, and output paths.
