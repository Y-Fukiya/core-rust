# Rust File Split Plan

Several Rust modules are intentionally large because Open Rules compatibility
work accumulated quickly in a few orchestration files. The current priority is
correctness and transparent scoring, but the next maintenance phase should split
the largest files without changing behavior.

## Current Hotspots

| File | Concern | Split direction |
| --- | --- | --- |
| `crates/core-api/src/lib.rs` | validation orchestration, Open Rules compatibility, metadata handling, and tests share one surface | split orchestration from Open Rules compatibility helpers |
| `crates/core-api/src/tests.rs` | broad regression coverage in one module | move tests by feature family |
| `crates/core-data/src/lib.rs` | CSV, XPT, package JSON, Open Rules data-dir loading, joins, and operations share one file | split loaders and dataset operations |
| `crates/core-engine/src/lib.rs` | operator evaluation and issue construction are coupled | split operators from validation result construction |
| `crates/core-rule-model/src/lib.rs` | file loading, schema normalization, YAML handling, and JSONATA normalization share one module | split loaders, metadata normalization, and JSONATA normalization |

## Order Of Work

1. Split `core-api` Open Rules compatibility helpers.
   - Move rule-id hand-port predicates and Open Rules oracle-gap helpers into an
     `open_rules_compat` module.
   - Keep public API stable: `run_validation`, `ValidateRequest`,
     `ValidateOutcome`, and `rule_uses_rule_id_hand_port`.
   - Run full workspace tests after each move.

2. Split `core-data` loaders.
   - Move generic CSV, Open Rules data-dir, XPT, and package JSON loading into
     separate modules.
   - Keep `LoadedDataset` and dataset operation APIs stable.
   - Add module-local tests only when moving existing tests makes ownership
     clearer.

3. Split `core-rule-model` normalization.
   - Separate file loading from CDISC metadata rule normalization.
   - Move JSONATA normalization into a focused module.
   - Keep `load_rule_file`, `load_rules_from_paths`, and `normalize_rule`
     stable.

4. Split `core-engine` operators.
   - Move operator-specific evaluation helpers into modules grouped by domain:
     comparison, string/regex, date/duration, set/relationship, and presence.
   - Keep `validate_rule` and `evaluate_condition_group` stable.

5. Split tests by feature family.
   - Prefer moving tests with the code they exercise.
   - Avoid broad fixture rewrites during the split.

## Guardrails

- No semantic changes in file-split commits.
- No baseline updates in the same commits unless generated output paths truly
  change.
- Run `cargo fmt --all -- --check`, `cargo check --workspace --locked`,
  `cargo clippy --workspace --locked -- -D warnings`, and
  `cargo test --workspace --locked` after each phase.
- Keep Open Rules fixture gate and baseline comparison green.
- Preserve git history by using move-only commits where practical.

## Exit Criteria

The split is successful when the large files become thin module entrypoints,
public APIs remain stable, and the full verification suite passes without
scoreboard or report-output regressions.
