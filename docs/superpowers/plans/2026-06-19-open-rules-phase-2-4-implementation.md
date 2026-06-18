# Open Rules Phase 2-4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the schema-aware Open Rules data loader, execution harness, baseline check, and CI path described in the Phase 2-4 design.

**Architecture:** Keep generic validation behavior unchanged. Add an Open Rules-specific loader in `core-data`, expose it through an explicit `ValidateRequest` loader mode in `core-api`, and orchestrate corpus runs from `xtask`. Reuse the Phase 1 scorer and mirrored candidate report layout.

**Tech Stack:** Rust 2021, clap, anyhow, csv, serde, serde_json, Polars, GitHub Actions, `cargo test --workspace`.

---

## File Structure

- Modify `crates/core-data/src/lib.rs`: add Open Rules data-dir loading, schema metadata parsing, conversion warnings, and tests.
- Modify `crates/core-api/src/lib.rs`: add `DatasetLoader` selection and route validation through the Open Rules loader when requested.
- Modify `xtask/Cargo.toml`: add workspace dependencies on `core-api`, `core-report`, and `core-engine` if needed for runner output.
- Modify `xtask/src/main.rs`: expose `open-rules run`, `open-rules run-score`, and `open-rules baseline`.
- Modify `xtask/src/open_rules/mod.rs`: wire new modules and public args.
- Create `xtask/src/open_rules/run.rs`: execute discovered cases through `core-api` and write mirrored reports.
- Create `xtask/src/open_rules/baseline.rs`: compare current scoreboards to `tests/open_rules/baseline.json`.
- Modify `xtask/src/open_rules/upstream.rs`: support strict lock checking through a reusable helper.
- Create `tests/fixtures/open_rules_executable`: small executable Open Rules-style fixture.
- Create `tests/open_rules/baseline.json`: accepted baseline for the executable fixture.
- Modify `.github/workflows/ci.yml`: add repository-local Open Rules run-score and baseline check.
- Modify `docs/open-rules-oracle-harness.md`: document Phase 2-4 operations.

## Task 1: Add Open Rules Loader Tests

**Files:**
- Modify: `crates/core-data/src/lib.rs`

- [ ] **Step 1: Add failing tests for schema-aware loading**

Add tests in the existing `#[cfg(test)] mod tests` in `crates/core-data/src/lib.rs`:

```rust
#[test]
fn load_open_rules_data_dir_uses_variables_schema() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("_datasets.csv"),
        "Filename,Label\ncm,Concomitant Medications\n",
    )
    .expect("datasets csv");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\nCM,CMTRT,Treatment,Char,40\n",
    )
    .expect("variables csv");
    fs::write(dir.path().join("cm.csv"), "CMSEQ,CMTRT\n001,ASPIRIN\n,PLACEBO\n")
        .expect("dataset csv");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert!(result.warnings.is_empty());
    let dataset = &result.datasets[0];
    assert_eq!(dataset.metadata.name, "CM");
    assert_eq!(dataset.metadata.domain.as_deref(), Some("CM"));
    assert_eq!(dataset.metadata.label.as_deref(), Some("Concomitant Medications"));
    assert_eq!(dataset.metadata.variables[0].name, "CMSEQ");
    assert_eq!(dataset.metadata.variables[0].variable_type.as_deref(), Some("Num"));
    assert_eq!(
        dataset_column_values(dataset, "CMSEQ").expect("CMSEQ values"),
        vec![
            serde_json::json!(1.0),
            serde_json::Value::Null,
        ]
    );
    assert_eq!(
        dataset_column_values(dataset, "CMTRT").expect("CMTRT values"),
        vec![serde_json::json!("ASPIRIN"), serde_json::json!("PLACEBO")]
    );
}
```

- [ ] **Step 2: Add warning and generic-loader preservation tests**

Add tests:

```rust
#[test]
fn load_open_rules_data_dir_warns_for_schema_mismatches() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("_datasets.csv"), "Filename,Label\ncm,CM\n").expect("datasets");
    fs::write(
        dir.path().join("_variables.csv"),
        "dataset,variable,label,type,length\nCM,CMSEQ,Sequence Number,Num,8\nCM,MISSING,Missing,Char,20\n",
    )
    .expect("variables");
    fs::write(dir.path().join("cm.csv"), "CMSEQ,EXTRA\nabc,value\n").expect("dataset");

    let result = load_open_rules_data_dir_with_warnings(dir.path()).expect("load open rules data");

    assert_eq!(result.datasets.len(), 1);
    assert_eq!(result.warnings.len(), 3);
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::InvalidNumericValue { .. }
    )));
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::DeclaredVariableMissing { .. }
    )));
    assert!(result.warnings.iter().any(|warning| matches!(
        warning.kind,
        LoadDataWarningKind::UndeclaredCsvColumn { .. }
    )));
}

#[test]
fn generic_csv_loader_still_infers_values() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("cm.csv");
    fs::write(&path, "CMSEQ\n001\n").expect("csv");

    let dataset = load_csv_dataset(&path).expect("load csv");

    assert_eq!(
        dataset_column_values(&dataset, "CMSEQ").expect("values"),
        vec![serde_json::json!("001")]
    );
}
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```sh
cargo test -p core-data load_open_rules_data_dir -- --nocapture
```

Expected: FAIL because `load_open_rules_data_dir_with_warnings` and the new warning variants are not defined.

## Task 2: Implement Open Rules Data Loader

**Files:**
- Modify: `crates/core-data/src/lib.rs`

- [ ] **Step 1: Add warning variants and public loader functions**

Add warning variants:

```rust
pub enum LoadDataWarningKind {
    UnsupportedExtension(String),
    InvalidNumericValue {
        dataset: String,
        variable: String,
        value: String,
        row: usize,
    },
    DeclaredVariableMissing {
        dataset: String,
        variable: String,
    },
    UndeclaredCsvColumn {
        dataset: String,
        variable: String,
    },
}
```

Add functions:

```rust
pub fn load_open_rules_data_dir(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>> {
    Ok(load_open_rules_data_dir_with_warnings(path)?.datasets)
}

pub fn load_open_rules_data_dir_with_warnings(path: impl AsRef<Path>) -> Result<LoadDataResult> {
    let path = path.as_ref();
    load_open_rules_data_dir_impl(path)
}
```

- [ ] **Step 2: Parse `_datasets.csv` and `_variables.csv` with flexible headers**

Implement helpers that read CSV rows into maps, accept `Filename`, `Dataset`, `Name`, `dataset`, `variable`, `type`, `length`, and `label`, and normalize dataset names by stripping `.csv` and uppercasing.

- [ ] **Step 3: Load each referenced dataset CSV with schema conversion**

Read dataset CSV using `csv::ReaderBuilder::new().flexible(true)`. For each CSV column:

- Find declared metadata by dataset and variable name.
- Convert numeric types to JSON numbers or null.
- Convert character types to JSON strings, preserving empty string.
- Emit `InvalidNumericValue` warnings for unparseable numeric cells.
- Emit `UndeclaredCsvColumn` warnings for columns absent from `_variables.csv`.

Then call existing `records_to_frame` to build the `DataFrame`.

- [ ] **Step 4: Preserve declared metadata**

Build `DatasetMetadata` with declared variables from `_variables.csv`, dataset label from `_datasets.csv`, canonical file path, and `DatasetSourceFormat::Csv`.

- [ ] **Step 5: Run tests and commit**

Run:

```sh
cargo test -p core-data load_open_rules_data_dir
cargo test -p core-data generic_csv_loader_still_infers_values
```

Expected: PASS.

Commit:

```sh
git add crates/core-data/src/lib.rs
git commit -m "Add schema-aware Open Rules data loader"
```

## Task 3: Add Loader Selection To `core-api`

**Files:**
- Modify: `crates/core-api/src/lib.rs`

- [ ] **Step 1: Add failing API test**

Add a unit test in `crates/core-api/src/lib.rs` that validates a rule against an Open Rules-style data directory and asserts that a numeric value with leading zero is treated numerically when `DatasetLoader::OpenRulesDataDir` is selected.

- [ ] **Step 2: Add `DatasetLoader` enum**

Add:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DatasetLoader {
    #[default]
    Generic,
    OpenRulesDataDir,
}
```

Add `pub dataset_loader: DatasetLoader` to `ValidateRequest`.

- [ ] **Step 3: Route dataset loading through selected loader**

Replace:

```rust
let datasets = load_datasets_from_paths(&request.dataset_paths)?;
```

with:

```rust
let datasets = match request.dataset_loader {
    DatasetLoader::Generic => load_datasets_from_paths(&request.dataset_paths)?,
    DatasetLoader::OpenRulesDataDir => load_open_rules_data_dirs(&request.dataset_paths)?,
};
```

`load_open_rules_data_dirs` should call `core_data::load_open_rules_data_dir` for each path.

- [ ] **Step 4: Run API tests and commit**

Run:

```sh
cargo test -p core-api
```

Expected: PASS.

Commit:

```sh
git add crates/core-api/src/lib.rs
git commit -m "Route validation through Open Rules data loader"
```

## Task 4: Add Executable Open Rules Fixture

**Files:**
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/rule.yml`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/positive/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/positive/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/positive/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/positive/01/results/results.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/negative/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/negative/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/negative/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_executable/Published/CORE-OPEN-0001/negative/01/results/results.csv`

- [ ] **Step 1: Create executable rule**

Use a CDISC metadata-style rule:

```yaml
Core:
  Id: CORE-OPEN-0001
  Status: Published
Scope:
  Domains: {}
  Classes: {}
Sensitivity: Record
Rule Type: Record Data
Check:
  all:
    - name: CMSEQ
      operator: greater_than
      value: 0
Outcome:
  Message: CMSEQ must be greater than zero
```

- [ ] **Step 2: Create positive and negative cases**

Positive `cm.csv`:

```csv
STUDYID,DOMAIN,USUBJID,CMSEQ,CMTRT
STUDY01,CM,SUBJ001,001,ASPIRIN
```

Negative `cm.csv`:

```csv
STUDYID,DOMAIN,USUBJID,CMSEQ,CMTRT
STUDY01,CM,SUBJ002,0,PLACEBO
```

Use `_variables.csv` with `CMSEQ` declared as `Num`.

- [ ] **Step 3: Create official results**

Positive `results.csv` should contain only the header:

```csv
rule_id,dataset,domain,row,variables,message,error_count,usubjid,seq
```

Negative `results.csv` should contain:

```csv
rule_id,dataset,domain,row,variables,message,error_count,usubjid,seq
CORE-OPEN-0001,CM,CM,1,CMSEQ,CMSEQ must be greater than zero,1,SUBJ002,0
```

- [ ] **Step 4: Commit fixture**

Run:

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root tests/fixtures/open_rules_executable \
  --core-rs-results-root tests/fixtures/open_rules_candidate_reports \
  --out target/open-rules-scoreboard-empty
```

Expected: FAIL because candidate reports are not generated yet.

Commit:

```sh
git add tests/fixtures/open_rules_executable
git commit -m "Add executable Open Rules fixture"
```

## Task 5: Implement `xtask open-rules run`

**Files:**
- Modify: `xtask/Cargo.toml`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/src/open_rules/mod.rs`
- Create: `xtask/src/open_rules/run.rs`

- [ ] **Step 1: Add dependencies**

Add to `xtask/Cargo.toml`:

```toml
core-api.workspace = true
core-report.workspace = true
```

- [ ] **Step 2: Add command args**

Add `RunArgs` with:

```rust
pub struct RunArgs {
    pub open_rules_root: PathBuf,
    pub core_rs_results_root: PathBuf,
    pub scope: Vec<String>,
}
```

- [ ] **Step 3: Implement per-case validation**

For each discovered case, call:

```rust
run_validation(ValidateRequest {
    rule_paths: vec![case.rule_path.clone()],
    dataset_paths: vec![case.data_dir.clone()],
    dataset_loader: DatasetLoader::OpenRulesDataDir,
    include_rules: vec![case.rule_id.clone()],
    output_format: ReportOutputFormat::Csv,
    output_dir: Some(output_dir),
    ..Default::default()
})
```

Use `relative_candidate_report_path(case).parent()` for output directory.

- [ ] **Step 4: Add tests**

Add tests proving `run_cases` writes:

```text
Published/CORE-OPEN-0001/positive/01/report.csv
Published/CORE-OPEN-0001/negative/01/report.csv
```

- [ ] **Step 5: Run tests and commit**

Run:

```sh
cargo test -p xtask open_rules
cargo run -p xtask -- open-rules run \
  --open-rules-root tests/fixtures/open_rules_executable \
  --core-rs-results-root target/open-rules-core-rs-fixture
```

Expected: PASS and candidate reports exist.

Commit:

```sh
git add xtask
git commit -m "Run core-rust against Open Rules cases"
```

## Task 6: Implement `run-score` And Strict Lock

**Files:**
- Modify: `xtask/src/main.rs`
- Modify: `xtask/src/open_rules/mod.rs`
- Modify: `xtask/src/open_rules/run.rs`
- Modify: `xtask/src/open_rules/score.rs`
- Modify: `xtask/src/open_rules/upstream.rs`

- [ ] **Step 1: Add `RunScoreArgs`**

Include:

```rust
pub struct RunScoreArgs {
    pub open_rules_root: PathBuf,
    pub core_rs_results_root: PathBuf,
    pub out: PathBuf,
    pub scope: Vec<String>,
    pub strict_lock: bool,
}
```

- [ ] **Step 2: Invoke run then score**

Implement `run_score(args)` by running cases and then calling the existing `score::run` with the same roots.

- [ ] **Step 3: Add strict-lock helper**

Add an upstream helper returning an error when expected and observed SHAs are present and differ.

- [ ] **Step 4: Run tests and commit**

Run:

```sh
cargo run -p xtask -- open-rules run-score \
  --open-rules-root tests/fixtures/open_rules_executable \
  --core-rs-results-root target/open-rules-core-rs-fixture \
  --out target/open-rules-scoreboard-fixture
```

Expected: PASS once official and candidate outputs match.

Commit:

```sh
git add xtask
git commit -m "Add Open Rules run-score command"
```

## Task 7: Implement Baseline Comparison

**Files:**
- Create: `xtask/src/open_rules/baseline.rs`
- Modify: `xtask/src/open_rules/mod.rs`
- Modify: `xtask/src/main.rs`
- Create: `tests/open_rules/baseline.json`

- [ ] **Step 1: Add baseline schema**

Use scoreboard JSON as the input schema and compare current case buckets against baseline case buckets by `scope/rule_id/case_kind/case_id`.

- [ ] **Step 2: Add command**

Expose:

```sh
cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard-fixture/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

- [ ] **Step 3: Create fixture baseline**

Generate the baseline from the executable fixture after `run-score` passes and commit it as `tests/open_rules/baseline.json`.

- [ ] **Step 4: Add tests**

Test that baseline comparison:

- passes for identical fixture scoreboard,
- fails when a `supported_match` baseline case becomes `supported_mismatch`,
- reports improvements without failing.

- [ ] **Step 5: Run tests and commit**

Run:

```sh
cargo test -p xtask baseline
cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard-fixture/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

Expected: PASS.

Commit:

```sh
git add xtask tests/open_rules/baseline.json
git commit -m "Add Open Rules baseline check"
```

## Task 8: Wire CI And Documentation

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/open-rules-oracle-harness.md`

- [ ] **Step 1: Add CI fixture check**

Add after `cargo test --workspace --locked`:

```yaml
      - name: Open Rules fixture oracle
        run: |
          cargo run -p xtask -- open-rules run-score \
            --open-rules-root tests/fixtures/open_rules_executable \
            --core-rs-results-root target/open-rules-core-rs-fixture \
            --out target/open-rules-scoreboard-fixture
          cargo run -p xtask -- open-rules baseline \
            --scoreboard target/open-rules-scoreboard-fixture/scoreboard.json \
            --baseline tests/open_rules/baseline.json
```

- [ ] **Step 2: Update docs**

Document:

- `load_open_rules_data_dir` behavior at a high level,
- `open-rules run`,
- `open-rules run-score`,
- `open-rules baseline`,
- `--strict-lock`,
- local full-corpus workflow using a separately checked out `cdisc-open-rules`.

- [ ] **Step 3: Run final verification and commit**

Run:

```sh
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo run -p xtask -- open-rules run-score \
  --open-rules-root tests/fixtures/open_rules_executable \
  --core-rs-results-root target/open-rules-core-rs-fixture \
  --out target/open-rules-scoreboard-fixture
cargo run -p xtask -- open-rules baseline \
  --scoreboard target/open-rules-scoreboard-fixture/scoreboard.json \
  --baseline tests/open_rules/baseline.json
```

Expected: all commands pass.

Commit:

```sh
git add .github/workflows/ci.yml docs/open-rules-oracle-harness.md
git commit -m "Document and verify Open Rules fixture CI"
```

## Self-Review

- Spec coverage: Phase 2 loader is covered by Tasks 1-3, Phase 3 execution by Tasks 4-6, Phase 4 baseline and CI by Tasks 7-8.
- Marker scan: No unresolved marker text or undefined follow-up work remains in this plan.
- Type consistency: `DatasetLoader::OpenRulesDataDir`, `load_open_rules_data_dir_with_warnings`, `open-rules run`, `open-rules run-score`, and `open-rules baseline` names are used consistently across tasks.
