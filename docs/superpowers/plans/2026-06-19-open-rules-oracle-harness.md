# Open Rules Oracle Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Phase 1 read-only CDISC Open Rules oracle harness as an `xtask` command that scores existing core-rust `report.csv` files against official `results.csv` files.

**Architecture:** Add a new `xtask` workspace crate with `open-rules score` as the only public command. Keep case discovery, CSV normalization, scoring, upstream lock handling, and report writing in focused modules under `xtask/src/open_rules`. The command never runs core-rust and never changes engine semantics.

**Tech Stack:** Rust 2021, clap, anyhow, csv, serde, serde_json, std filesystem APIs, `cargo test --workspace`.

---

## File Structure

- Create `xtask/Cargo.toml`: package metadata and dependencies for the development tool.
- Create `xtask/src/main.rs`: clap CLI entrypoint with `open-rules score`.
- Create `xtask/src/open_rules/mod.rs`: module wiring and command boundary.
- Create `xtask/src/open_rules/discovery.rs`: scan a local `cdisc-open-rules` checkout and build case metadata.
- Create `xtask/src/open_rules/normalize.rs`: convert official and candidate CSV rows into comparable issue keys.
- Create `xtask/src/open_rules/score.rs`: classify cases into `supported_match`, `supported_mismatch`, `skipped_unsupported`, or `harness_error`.
- Create `xtask/src/open_rules/report.rs`: write `scoreboard.json` and `summary.md`.
- Create `xtask/src/open_rules/upstream.rs`: read `tests/open_rules/upstream.lock` and local checkout SHA.
- Modify `Cargo.toml`: add `xtask` to workspace members.
- Create `tests/open_rules/upstream.lock`: initial pinned upstream metadata.
- Create `tests/fixtures/open_rules_minimal`: minimal official corpus fixture.
- Create `tests/fixtures/open_rules_candidate_reports`: mirrored candidate report fixture.
- Create `docs/open-rules-oracle-harness.md`: user-facing harness documentation.
- Create `AGENTS.md`: concise coding-agent guardrails for oracle compatibility work.

## Task 1: Scaffold `xtask`

**Files:**
- Modify: `Cargo.toml`
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`
- Create: `xtask/src/open_rules/mod.rs`
- Create: `xtask/src/open_rules/score.rs`
- Create: `xtask/src/open_rules/discovery.rs`
- Create: `xtask/src/open_rules/normalize.rs`
- Create: `xtask/src/open_rules/report.rs`
- Create: `xtask/src/open_rules/upstream.rs`

- [ ] **Step 1: Verify the package is absent before scaffolding**

Run:

```sh
cargo check -p xtask
```

Expected: FAIL with a package-not-found error for `xtask`.

- [ ] **Step 2: Add `xtask` to the workspace**

Edit the root `Cargo.toml` workspace members to include `xtask`:

```toml
[workspace]
members = [
    "apps/cli",
    "crates/core-api",
    "crates/core-cdisc-library",
    "crates/core-data",
    "crates/core-engine",
    "crates/core-report",
    "crates/core-rule-model",
    "xtask",
]
resolver = "2"
```

- [ ] **Step 3: Create `xtask/Cargo.toml`**

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
anyhow.workspace = true
clap.workspace = true
csv.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true
tempfile.workspace = true
```

- [ ] **Step 4: Create the CLI skeleton in `xtask/src/main.rs`**

```rust
#![forbid(unsafe_code)]

mod open_rules;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Development tasks for core-rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// CDISC Open Rules compatibility tooling.
    OpenRules(OpenRulesCommand),
}

#[derive(Debug, Parser)]
struct OpenRulesCommand {
    #[command(subcommand)]
    command: OpenRulesSubcommand,
}

#[derive(Debug, Subcommand)]
enum OpenRulesSubcommand {
    /// Score existing core-rust reports against official Open Rules results.
    Score(open_rules::ScoreArgs),
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let should_fail = match cli.command {
        Commands::OpenRules(command) => match command.command {
            OpenRulesSubcommand::Score(args) => open_rules::score(args)?,
        },
    };

    Ok(if should_fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
```

- [ ] **Step 5: Create module wiring in `xtask/src/open_rules/mod.rs`**

```rust
pub mod discovery;
pub mod normalize;
pub mod report;
pub mod score;
pub mod upstream;

pub use score::ScoreArgs;

pub fn score(args: ScoreArgs) -> anyhow::Result<bool> {
    score::run(args)
}
```

- [ ] **Step 6: Create a compiling score stub in `xtask/src/open_rules/score.rs`**

```rust
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct ScoreArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,
}

pub fn run(_args: ScoreArgs) -> anyhow::Result<bool> {
    Ok(false)
}
```

- [ ] **Step 7: Create empty module files that compile**

Create each file with a module-level comment:

```rust
//! Open Rules case discovery.
```

Use that pattern for:

```text
xtask/src/open_rules/discovery.rs
xtask/src/open_rules/normalize.rs
xtask/src/open_rules/report.rs
xtask/src/open_rules/upstream.rs
```

- [ ] **Step 8: Verify the scaffold**

Run:

```sh
cargo check -p xtask
```

Expected: PASS.

- [ ] **Step 9: Commit the scaffold**

```sh
git add Cargo.toml xtask
git commit -m "Add xtask scaffold for open rules harness"
```

## Task 2: Add Minimal Fixtures And Upstream Lock

**Files:**
- Create: `tests/open_rules/upstream.lock`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/rule.yml`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/positive/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/positive/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/positive/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/positive/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/positive/01/results/results.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/negative/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/negative/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/negative/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/negative/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000001/negative/01/results/results.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/rule.yml`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/negative/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/negative/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/negative/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/negative/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000002/negative/01/results/results.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000003/rule.yml`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000003/positive/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000003/positive/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000003/positive/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000003/positive/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/rule.yml`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/negative/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/negative/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/negative/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/negative/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000004/negative/01/results/results.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/rule.yml`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/negative/01/data/.env`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/negative/01/data/_datasets.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/negative/01/data/_variables.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/negative/01/data/cm.csv`
- Create: `tests/fixtures/open_rules_minimal/Published/CORE-000005/negative/01/results/results.csv`
- Create: candidate reports under `tests/fixtures/open_rules_candidate_reports/Published/...`

- [ ] **Step 1: Create `tests/open_rules/upstream.lock`**

Use the current upstream main SHA verified on 2026-06-19:

```text
repo=https://github.com/cdisc-org/cdisc-open-rules.git
sha=7f7fae49376b3d023563ebb6c36a3b392d6e649f
```

- [ ] **Step 2: Create shared official fixture content**

Use this `rule.yml` for every fixture rule, changing only `core_id`:

```yaml
core_id: CORE-000001
description: Minimal oracle fixture rule
```

Use this `.env` for every fixture case:

```text
PRODUCT=SDTMIG
VERSION=3-4
SUBSTANDARD=SDTM
USE_CASE=TEST
```

Use this `_datasets.csv` for every fixture case:

```csv
Filename,Label
cm,Concomitant Medications
```

Use this `_variables.csv` for every fixture case:

```csv
dataset,variable,label,type,length
CM,STUDYID,Study Identifier,Char,50
CM,DOMAIN,Domain Abbreviation,Char,2
CM,USUBJID,Unique Subject Identifier,Char,50
CM,CMSEQ,Sequence Number,Num,8
CM,CMTRT,Reported Name of Drug,Char,200
```

Use this `cm.csv` for every fixture case:

```csv
STUDYID,DOMAIN,USUBJID,CMSEQ,CMTRT
STUDY01,CM,STUDY01-001,1,PLACEBO
STUDY01,CM,STUDY01-002,2,ASPIRIN
```

- [ ] **Step 3: Create official result files**

For `CORE-000001/positive/01/results/results.csv`:

```csv
rule_id,dataset,domain,row,variables,usubjid,seq,message
```

For `CORE-000001/negative/01/results/results.csv`:

```csv
rule_id,dataset,domain,row,variables,usubjid,seq,message
CORE-000001,CM,CM,2,CMTRT|CMSEQ,STUDY01-002,2,Official message differs
```

For `CORE-000002/negative/01/results/results.csv`:

```csv
rule_id,dataset,domain,row,variables,usubjid,seq,message
CORE-000002,CM,CM,2,CMTRT,STUDY01-002,2,Official skipped fixture issue
```

Do not create `CORE-000003/positive/01/results/results.csv`; this case verifies missing official output becomes `harness_error`.

For `CORE-000004/negative/01/results/results.csv`:

```csv
rule_id,dataset,domain,row,variables,usubjid,seq,message
CORE-000004,CM,CM,2,CMTRT,STUDY01-002,2,Official missing candidate fixture issue
```

For `CORE-000005/negative/01/results/results.csv`:

```csv
rule_id,dataset,domain,row,variables,usubjid,seq,message
CORE-000005,CM,CM,2,CMTRT,STUDY01-002,2,Official mismatch fixture issue
```

- [ ] **Step 4: Create mirrored candidate reports**

For `tests/fixtures/open_rules_candidate_reports/Published/CORE-000001/positive/01/report.csv`:

```csv
rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq
CORE-000001,passed,CM,CM,,,Candidate passed message,0,,,
```

For `tests/fixtures/open_rules_candidate_reports/Published/CORE-000001/negative/01/report.csv`:

```csv
rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq
CORE-000001,failed,CM,CM,2,CMSEQ|CMTRT,Candidate message differs,1,,STUDY01-002,2
```

For `tests/fixtures/open_rules_candidate_reports/Published/CORE-000002/negative/01/report.csv`:

```csv
rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq
CORE-000002,skipped,,,,,unsupported operator fixture,0,unsupported_operator,,
```

For `tests/fixtures/open_rules_candidate_reports/Published/CORE-000003/positive/01/report.csv`:

```csv
rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq
CORE-000003,passed,CM,CM,,,Candidate passed message,0,,,
```

Do not create a candidate report for `CORE-000004/negative/01`; this case verifies missing candidate output becomes `harness_error`.

For `tests/fixtures/open_rules_candidate_reports/Published/CORE-000005/negative/01/report.csv`:

```csv
rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq
CORE-000005,failed,CM,CM,1,CMTRT,Candidate mismatch fixture issue,1,,STUDY01-001,1
```

- [ ] **Step 5: Verify fixture files are visible**

Run:

```sh
rg --files tests/fixtures/open_rules_minimal tests/fixtures/open_rules_candidate_reports tests/open_rules
```

Expected: paths for the upstream lock, official fixture tree, and candidate report tree.

- [ ] **Step 6: Commit the fixtures**

```sh
git add tests/open_rules tests/fixtures/open_rules_minimal tests/fixtures/open_rules_candidate_reports
git commit -m "Add minimal open rules oracle fixtures"
```

## Task 3: Implement Case Discovery

**Files:**
- Modify: `xtask/src/open_rules/discovery.rs`

- [ ] **Step 1: Write failing discovery tests**

Add these tests at the bottom of `xtask/src/open_rules/discovery.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::*;

    fn fixture_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests/fixtures/open_rules_minimal")
    }

    #[test]
    fn discovers_published_cases_by_default() {
        let cases = discover_cases(&fixture_root(), &[]).expect("discover cases");

        assert_eq!(cases.len(), 6);
        assert_eq!(cases[0].scope, "Published");
        assert_eq!(cases[0].rule_id, "CORE-000001");
        assert_eq!(cases[0].case_kind, CaseKind::Negative);
        assert_eq!(cases[0].case_id, "01");
        assert!(cases[0].rule_path.ends_with("rule.yml"));
        assert!(cases[0].data_dir.ends_with("data"));
        assert_eq!(cases[0].env.get("PRODUCT").map(String::as_str), Some("SDTMIG"));
        assert_eq!(cases[0].datasets.len(), 1);
        assert_eq!(cases[0].variables.len(), 5);
        assert_eq!(cases[0].dataset_files.len(), 1);
        assert!(cases[0].dataset_files[0].ends_with("cm.csv"));
    }

    #[test]
    fn reports_missing_official_results_without_dropping_case() {
        let cases = discover_cases(&fixture_root(), &[]).expect("discover cases");
        let missing = cases
            .iter()
            .find(|case| case.rule_id == "CORE-000003")
            .expect("CORE-000003 case");

        assert!(!missing.has_official_results);
        assert!(missing.official_results_csv.ends_with("results.csv"));
    }
}
```

- [ ] **Step 2: Run the discovery tests to verify they fail**

Run:

```sh
cargo test -p xtask open_rules::discovery
```

Expected: FAIL because `discover_cases`, `OpenRulesCase`, and `CaseKind` are not defined.

- [ ] **Step 3: Implement discovery types and parsing**

Replace `xtask/src/open_rules/discovery.rs` with:

```rust
//! Open Rules case discovery.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    Positive,
    Negative,
}

impl CaseKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenRulesCase {
    pub scope: String,
    pub rule_id: String,
    pub rule_dir: PathBuf,
    pub rule_path: PathBuf,
    pub case_kind: CaseKind,
    pub case_id: String,
    pub case_dir: PathBuf,
    pub data_dir: PathBuf,
    pub env_path: PathBuf,
    pub env: BTreeMap<String, String>,
    pub datasets_path: PathBuf,
    pub datasets: Vec<BTreeMap<String, String>>,
    pub dataset_files: Vec<PathBuf>,
    pub variables_path: PathBuf,
    pub variables: Vec<BTreeMap<String, String>>,
    pub official_results_csv: PathBuf,
    pub has_official_results: bool,
}

pub fn discover_cases(open_rules_root: &Path, scopes: &[String]) -> Result<Vec<OpenRulesCase>> {
    let scopes = if scopes.is_empty() {
        vec!["Published".to_owned()]
    } else {
        scopes.to_vec()
    };

    let mut cases = Vec::new();
    for scope in scopes {
        let scope_dir = open_rules_root.join(&scope);
        if !scope_dir.exists() {
            continue;
        }
        discover_scope(&scope, &scope_dir, &mut cases)?;
    }
    cases.sort_by(|left, right| {
        (
            &left.scope,
            &left.rule_id,
            left.case_kind,
            &left.case_id,
        )
            .cmp(&(
                &right.scope,
                &right.rule_id,
                right.case_kind,
                &right.case_id,
            ))
    });
    Ok(cases)
}

fn discover_scope(scope: &str, scope_dir: &Path, cases: &mut Vec<OpenRulesCase>) -> Result<()> {
    let mut stack = vec![scope_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children = sorted_children(&dir)?;
        children.reverse();
        for child in children {
            if child.is_dir() {
                stack.push(child);
            }
        }

        let rule_path = dir.join("rule.yml");
        if rule_path.is_file() {
            discover_rule_cases(scope, &dir, &rule_path, cases)?;
        }
    }
    Ok(())
}

fn discover_rule_cases(
    scope: &str,
    rule_dir: &Path,
    rule_path: &Path,
    cases: &mut Vec<OpenRulesCase>,
) -> Result<()> {
    let rule_id = rule_dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("rule directory name is not valid UTF-8")?
        .to_owned();

    for case_kind in [CaseKind::Positive, CaseKind::Negative] {
        let kind_dir = rule_dir.join(case_kind.as_str());
        if !kind_dir.is_dir() {
            continue;
        }
        for case_dir in sorted_children(&kind_dir)? {
            if !case_dir.is_dir() {
                continue;
            }
            let case_id = case_dir
                .file_name()
                .and_then(|name| name.to_str())
                .context("case directory name is not valid UTF-8")?
                .to_owned();
            let data_dir = case_dir.join("data");
            let env_path = data_dir.join(".env");
            let datasets_path = data_dir.join("_datasets.csv");
            let variables_path = data_dir.join("_variables.csv");
            let official_results_csv = case_dir.join("results").join("results.csv");
            let datasets = read_csv_dicts(&datasets_path)?;
            let variables = read_csv_dicts(&variables_path)?;
            let dataset_files = datasets
                .iter()
                .filter_map(dataset_filename)
                .map(|name| data_dir.join(format!("{}.csv", strip_csv_suffix(&name))))
                .collect::<Vec<_>>();

            cases.push(OpenRulesCase {
                scope: scope.to_owned(),
                rule_id: rule_id.clone(),
                rule_dir: rule_dir.to_path_buf(),
                rule_path: rule_path.to_path_buf(),
                case_kind,
                case_id,
                case_dir,
                data_dir,
                env_path: env_path.clone(),
                env: read_env_file(&env_path)?,
                datasets_path,
                datasets,
                dataset_files,
                variables_path,
                variables,
                has_official_results: official_results_csv.is_file(),
                official_results_csv,
            });
        }
    }

    Ok(())
}

fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read directory {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("read directory entry in {}", dir.display()))?;
    entries.sort();
    Ok(entries)
}

fn read_env_file(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let source = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let values = source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_owned(),
                value.trim().trim_matches('"').trim_matches('\'').to_owned(),
            ))
        })
        .collect();
    Ok(values)
}

fn read_csv_dicts(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("open CSV {}", path.display()))?;
    let headers = reader
        .headers()
        .with_context(|| format!("read CSV headers {}", path.display()))?
        .iter()
        .map(|header| header.trim().to_owned())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.with_context(|| format!("read CSV record {}", path.display()))?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.clone(), value.trim().to_owned()))
            .collect::<BTreeMap<_, _>>();
        rows.push(row);
    }
    Ok(rows)
}

fn dataset_filename(row: &BTreeMap<String, String>) -> Option<String> {
    ["Filename", "filename", "Dataset", "dataset", "Name", "name"]
        .iter()
        .find_map(|key| row.get(*key))
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn strip_csv_suffix(value: &str) -> String {
    value
        .trim()
        .strip_suffix(".csv")
        .or_else(|| value.trim().strip_suffix(".CSV"))
        .unwrap_or_else(|| value.trim())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::*;

    fn fixture_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests/fixtures/open_rules_minimal")
    }

    #[test]
    fn discovers_published_cases_by_default() {
        let cases = discover_cases(&fixture_root(), &[]).expect("discover cases");

        assert_eq!(cases.len(), 6);
        assert_eq!(cases[0].scope, "Published");
        assert_eq!(cases[0].rule_id, "CORE-000001");
        assert_eq!(cases[0].case_kind, CaseKind::Negative);
        assert_eq!(cases[0].case_id, "01");
        assert!(cases[0].rule_path.ends_with("rule.yml"));
        assert!(cases[0].data_dir.ends_with("data"));
        assert_eq!(cases[0].env.get("PRODUCT").map(String::as_str), Some("SDTMIG"));
        assert_eq!(cases[0].datasets.len(), 1);
        assert_eq!(cases[0].variables.len(), 5);
        assert_eq!(cases[0].dataset_files.len(), 1);
        assert!(cases[0].dataset_files[0].ends_with("cm.csv"));
    }

    #[test]
    fn reports_missing_official_results_without_dropping_case() {
        let cases = discover_cases(&fixture_root(), &[]).expect("discover cases");
        let missing = cases
            .iter()
            .find(|case| case.rule_id == "CORE-000003")
            .expect("CORE-000003 case");

        assert!(!missing.has_official_results);
        assert!(missing.official_results_csv.ends_with("results.csv"));
    }
}
```

- [ ] **Step 4: Run discovery tests**

Run:

```sh
cargo test -p xtask open_rules::discovery
```

Expected: PASS.

- [ ] **Step 5: Commit discovery**

```sh
git add xtask/src/open_rules/discovery.rs
git commit -m "Discover open rules oracle cases"
```

## Task 4: Implement CSV Normalization

**Files:**
- Modify: `xtask/src/open_rules/normalize.rs`

- [ ] **Step 1: Write failing normalizer tests**

Add these tests to `xtask/src/open_rules/normalize.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn normalizes_issue_keys_without_messages_or_variable_order() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,USUBJID,SEQ,Message\ncm.csv,2,CMTRT|CMSEQ,STUDY01-002,2,official text\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,failed,CM,CM,2,CMSEQ|CMTRT,candidate text,1,,STUDY01-002,2\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000001"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000001"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
        assert_eq!(official.issues[0].dataset, "CM");
        assert_eq!(official.issues[0].variables, vec!["CMSEQ", "CMTRT"]);
    }

    #[test]
    fn core_rs_passed_and_skipped_rows_are_not_issue_keys() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("report.csv");
        fs::write(
            &report,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,passed,CM,CM,,,passed,0,,,\nCORE-000002,skipped,,,,,unsupported,0,unsupported_operator,,\n",
        )
        .expect("write report");

        let normalized = normalize_csv(&report, ReportSource::CoreRs, None).expect("normalize");

        assert_eq!(normalized.row_count, 2);
        assert_eq!(normalized.skipped_row_count, 1);
        assert_eq!(normalized.issue_count, 0);
        assert!(normalized.issues.is_empty());
    }

    #[test]
    fn nullish_values_normalize_to_empty_but_zero_and_dot_remain() {
        assert_eq!(normalize_scalar(" null "), "");
        assert_eq!(normalize_scalar("N/A"), "");
        assert_eq!(normalize_scalar("0"), "0");
        assert_eq!(normalize_scalar("."), ".");
    }
}
```

- [ ] **Step 2: Run normalizer tests to verify failure**

Run:

```sh
cargo test -p xtask open_rules::normalize
```

Expected: FAIL because `normalize_csv`, `ReportSource`, and `normalize_scalar` are not defined.

- [ ] **Step 3: Implement normalization**

Replace `xtask/src/open_rules/normalize.rs` with:

```rust
//! Normalize official CORE and core-rust CSV reports to structural issue keys.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSource {
    Official,
    CoreRs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct IssueKey {
    pub rule_id: String,
    pub dataset: String,
    pub domain: String,
    pub row: String,
    pub variables: Vec<String>,
    pub usubjid: String,
    pub seq: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedCsv {
    pub path: PathBuf,
    pub row_count: usize,
    pub skipped_row_count: usize,
    pub issue_count: usize,
    pub issues: Vec<IssueKey>,
}

pub fn normalize_csv(
    path: &Path,
    source: ReportSource,
    default_rule_id: Option<&str>,
) -> Result<NormalizedCsv> {
    let rows = read_rows(path)?;
    let skipped_row_count = match source {
        ReportSource::Official => 0,
        ReportSource::CoreRs => rows.iter().filter(|row| row_is_core_rs_skipped(row)).count(),
    };
    let issue_rows = rows
        .iter()
        .filter(|row| match source {
            ReportSource::Official => true,
            ReportSource::CoreRs => !row_is_core_rs_non_issue(row),
        })
        .collect::<Vec<_>>();
    let issues = issue_rows
        .into_iter()
        .map(|row| normalize_row(row, default_rule_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Ok(NormalizedCsv {
        path: path.to_path_buf(),
        row_count: rows.len(),
        skipped_row_count,
        issue_count: issues.len(),
        issues,
    })
}

pub fn normalize_scalar(value: &str) -> String {
    let text = value.trim();
    if matches!(
        text.to_ascii_lowercase().as_str(),
        "" | "null" | "none" | "nan" | "na" | "n/a"
    ) {
        String::new()
    } else {
        text.to_owned()
    }
}

fn read_rows(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("open CSV {}", path.display()))?;
    let headers = reader
        .headers()
        .with_context(|| format!("read CSV headers {}", path.display()))?
        .iter()
        .map(|header| header.trim().to_owned())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.with_context(|| format!("read CSV record {}", path.display()))?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.clone(), value.to_owned()))
            .collect::<BTreeMap<_, _>>();
        rows.push(row);
    }
    Ok(rows)
}

fn normalize_row(row: &BTreeMap<String, String>, default_rule_id: Option<&str>) -> IssueKey {
    let rule_id = first(row, &["rule_id", "rule", "core_id", "core-id", "id"])
        .or_else(|| default_rule_id.map(str::to_owned))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let dataset = normalize_dataset_like(
        &first(
            row,
            &["dataset", "dataset_name", "Dataset", "domain", "domain_name"],
        )
        .unwrap_or_default(),
    );
    let domain = normalize_dataset_like(
        &first(row, &["domain", "domain_name"])
            .unwrap_or_else(|| dataset.clone()),
    );
    let row_number = first(
        row,
        &["row", "row_number", "record", "Record", "record_number", "line", "line_number"],
    )
    .unwrap_or_default();
    let variables = split_variables(
        &first(
            row,
            &["variables", "variable", "Variable", "variable_name", "column", "columns"],
        )
        .unwrap_or_default(),
    );
    let usubjid = first(row, &["usubjid", "USUBJID", "subject", "subject_id"]).unwrap_or_default();
    let seq = first(row, &["seq", "SEQ", "sequence", "sequence_number"]).unwrap_or_default();

    IssueKey {
        rule_id,
        dataset,
        domain,
        row: normalize_scalar(&row_number),
        variables,
        usubjid: normalize_scalar(&usubjid),
        seq: normalize_scalar(&seq),
    }
}

fn first(row: &BTreeMap<String, String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        row.iter()
            .find(|(key, _value)| key.trim().eq_ignore_ascii_case(name))
            .map(|(_key, value)| normalize_scalar(value))
            .filter(|value| !value.is_empty())
    })
}

fn normalize_dataset_like(value: &str) -> String {
    let value = normalize_scalar(value);
    let value = value
        .strip_suffix(".csv")
        .or_else(|| value.strip_suffix(".CSV"))
        .unwrap_or(value.as_str());
    value.to_ascii_uppercase()
}

fn split_variables(value: &str) -> Vec<String> {
    let value = normalize_scalar(value);
    if value.is_empty() {
        return Vec::new();
    }
    value
        .split(['|', ';', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_uppercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn row_is_core_rs_skipped(row: &BTreeMap<String, String>) -> bool {
    let status = first(row, &["execution_status", "status"]).unwrap_or_default();
    let skipped_reason = first(row, &["skipped_reason", "skip_reason"]).unwrap_or_default();
    status.eq_ignore_ascii_case("skipped") || !skipped_reason.is_empty()
}

fn row_is_core_rs_non_issue(row: &BTreeMap<String, String>) -> bool {
    let status = first(row, &["execution_status", "status"]).unwrap_or_default();
    status.eq_ignore_ascii_case("passed") || status.eq_ignore_ascii_case("skipped")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn normalizes_issue_keys_without_messages_or_variable_order() {
        let dir = tempdir().expect("tempdir");
        let official = dir.path().join("official.csv");
        let candidate = dir.path().join("candidate.csv");
        fs::write(
            &official,
            "Dataset,Record,Variable,USUBJID,SEQ,Message\ncm.csv,2,CMTRT|CMSEQ,STUDY01-002,2,official text\n",
        )
        .expect("write official");
        fs::write(
            &candidate,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,failed,CM,CM,2,CMSEQ|CMTRT,candidate text,1,,STUDY01-002,2\n",
        )
        .expect("write candidate");

        let official = normalize_csv(&official, ReportSource::Official, Some("CORE-000001"))
            .expect("official");
        let candidate = normalize_csv(&candidate, ReportSource::CoreRs, Some("CORE-000001"))
            .expect("candidate");

        assert_eq!(official.issues, candidate.issues);
        assert_eq!(official.issues[0].dataset, "CM");
        assert_eq!(official.issues[0].variables, vec!["CMSEQ", "CMTRT"]);
    }

    #[test]
    fn core_rs_passed_and_skipped_rows_are_not_issue_keys() {
        let dir = tempdir().expect("tempdir");
        let report = dir.path().join("report.csv");
        fs::write(
            &report,
            "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\nCORE-000001,passed,CM,CM,,,passed,0,,,\nCORE-000002,skipped,,,,,unsupported,0,unsupported_operator,,\n",
        )
        .expect("write report");

        let normalized = normalize_csv(&report, ReportSource::CoreRs, None).expect("normalize");

        assert_eq!(normalized.row_count, 2);
        assert_eq!(normalized.skipped_row_count, 1);
        assert_eq!(normalized.issue_count, 0);
        assert!(normalized.issues.is_empty());
    }

    #[test]
    fn nullish_values_normalize_to_empty_but_zero_and_dot_remain() {
        assert_eq!(normalize_scalar(" null "), "");
        assert_eq!(normalize_scalar("N/A"), "");
        assert_eq!(normalize_scalar("0"), "0");
        assert_eq!(normalize_scalar("."), ".");
    }
}
```

- [ ] **Step 4: Run normalizer tests**

Run:

```sh
cargo test -p xtask open_rules::normalize
```

Expected: PASS.

- [ ] **Step 5: Commit normalization**

```sh
git add xtask/src/open_rules/normalize.rs
git commit -m "Normalize open rules report issue keys"
```

## Task 5: Implement Case Scoring

**Files:**
- Modify: `xtask/src/open_rules/score.rs`

- [ ] **Step 1: Write failing scoring tests**

Add this test module to `xtask/src/open_rules/score.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use crate::open_rules::discovery::discover_cases;

    use super::*;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn scores_match_mismatch_skip_and_harness_errors() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let candidate_root = repo_root().join("tests/fixtures/open_rules_candidate_reports");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");

        let scored = score_cases(&cases, &candidate_root);
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(summary.total_cases, 6);
        assert_eq!(summary.supported_match, 2);
        assert_eq!(summary.supported_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 1);
        assert_eq!(summary.harness_error, 2);
        assert_eq!(summary.supported_accuracy, Some(2.0 / 3.0));
        assert_eq!(summary.coverage, Some(3.0 / 6.0));
        assert!(summary.should_fail());
    }

    #[test]
    fn candidate_report_path_mirrors_case_identity() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");
        let case = cases
            .iter()
            .find(|case| case.rule_id == "CORE-000001" && case.case_kind.as_str() == "positive")
            .expect("positive case");

        assert_eq!(
            relative_candidate_report_path(case),
            Path::new("Published")
                .join("CORE-000001")
                .join("positive")
                .join("01")
                .join("report.csv")
        );
    }
}
```

- [ ] **Step 2: Run scoring tests to verify failure**

Run:

```sh
cargo test -p xtask open_rules::score
```

Expected: FAIL because scoring types and functions are not defined.

- [ ] **Step 3: Implement scoring**

Replace `xtask/src/open_rules/score.rs` with:

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::open_rules::discovery::OpenRulesCase;
use crate::open_rules::normalize::{normalize_csv, IssueKey, ReportSource};

#[derive(Debug, Clone, Parser)]
pub struct ScoreArgs {
    #[arg(long, value_name = "DIR")]
    pub open_rules_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub core_rs_results_root: PathBuf,

    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,

    #[arg(long, value_name = "SCOPE")]
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBucket {
    SupportedMatch,
    SupportedMismatch,
    SkippedUnsupported,
    HarnessError,
}

impl ScoreBucket {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SupportedMatch => "supported_match",
            Self::SupportedMismatch => "supported_mismatch",
            Self::SkippedUnsupported => "skipped_unsupported",
            Self::HarnessError => "harness_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoredCase {
    pub scope: String,
    pub rule_id: String,
    pub case_kind: String,
    pub case_id: String,
    pub case_dir: PathBuf,
    pub official_results_csv: PathBuf,
    pub candidate_report_csv: PathBuf,
    pub bucket: ScoreBucket,
    pub reason: Option<String>,
    pub official_issue_count: Option<usize>,
    pub candidate_issue_count: Option<usize>,
    pub missing: Vec<IssueKey>,
    pub extra: Vec<IssueKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreSummary {
    pub total_cases: usize,
    pub supported_match: usize,
    pub supported_mismatch: usize,
    pub skipped_unsupported: usize,
    pub harness_error: usize,
    pub supported_accuracy: Option<f64>,
    pub coverage: Option<f64>,
}

pub fn run(_args: ScoreArgs) -> anyhow::Result<bool> {
    Ok(false)
}

pub fn score_cases(cases: &[OpenRulesCase], core_rs_results_root: &Path) -> Vec<ScoredCase> {
    cases
        .iter()
        .map(|case| score_case(case, core_rs_results_root))
        .collect()
}

pub fn relative_candidate_report_path(case: &OpenRulesCase) -> PathBuf {
    Path::new(&case.scope)
        .join(&case.rule_id)
        .join(case.case_kind.as_str())
        .join(&case.case_id)
        .join("report.csv")
}

fn score_case(case: &OpenRulesCase, core_rs_results_root: &Path) -> ScoredCase {
    let candidate_report_csv = core_rs_results_root.join(relative_candidate_report_path(case));
    let base = ScoredCase {
        scope: case.scope.clone(),
        rule_id: case.rule_id.clone(),
        case_kind: case.case_kind.as_str().to_owned(),
        case_id: case.case_id.clone(),
        case_dir: case.case_dir.clone(),
        official_results_csv: case.official_results_csv.clone(),
        candidate_report_csv: candidate_report_csv.clone(),
        bucket: ScoreBucket::HarnessError,
        reason: None,
        official_issue_count: None,
        candidate_issue_count: None,
        missing: Vec::new(),
        extra: Vec::new(),
    };

    if !case.official_results_csv.is_file() {
        return ScoredCase {
            reason: Some("missing official results.csv".to_owned()),
            ..base
        };
    }
    if !candidate_report_csv.is_file() {
        return ScoredCase {
            reason: Some("missing candidate report.csv".to_owned()),
            ..base
        };
    }

    let official = match normalize_csv(
        &case.official_results_csv,
        ReportSource::Official,
        Some(&case.rule_id),
    ) {
        Ok(normalized) => normalized,
        Err(source) => {
            return ScoredCase {
                reason: Some(format!("official normalization error: {source}")),
                ..base
            }
        }
    };
    let candidate = match normalize_csv(
        &candidate_report_csv,
        ReportSource::CoreRs,
        Some(&case.rule_id),
    ) {
        Ok(normalized) => normalized,
        Err(source) => {
            return ScoredCase {
                reason: Some(format!("candidate normalization error: {source}")),
                ..base
            }
        }
    };

    if candidate.skipped_row_count > 0 {
        return ScoredCase {
            bucket: ScoreBucket::SkippedUnsupported,
            reason: Some("candidate output contains skipped rows".to_owned()),
            official_issue_count: Some(official.issue_count),
            candidate_issue_count: Some(candidate.issue_count),
            ..base
        };
    }

    let official_set = official.issues.into_iter().collect::<std::collections::BTreeSet<_>>();
    let candidate_set = candidate.issues.into_iter().collect::<std::collections::BTreeSet<_>>();
    let missing = official_set
        .difference(&candidate_set)
        .cloned()
        .collect::<Vec<_>>();
    let extra = candidate_set
        .difference(&official_set)
        .cloned()
        .collect::<Vec<_>>();
    let bucket = if missing.is_empty() && extra.is_empty() {
        ScoreBucket::SupportedMatch
    } else {
        ScoreBucket::SupportedMismatch
    };

    ScoredCase {
        bucket,
        official_issue_count: Some(official_set.len()),
        candidate_issue_count: Some(candidate_set.len()),
        missing,
        extra,
        ..base
    }
}

impl ScoreSummary {
    pub fn from_cases(cases: &[ScoredCase]) -> Self {
        let mut counts = BTreeMap::<&'static str, usize>::new();
        for case in cases {
            *counts.entry(case.bucket.as_str()).or_default() += 1;
        }
        let supported_match = *counts.get("supported_match").unwrap_or(&0);
        let supported_mismatch = *counts.get("supported_mismatch").unwrap_or(&0);
        let skipped_unsupported = *counts.get("skipped_unsupported").unwrap_or(&0);
        let harness_error = *counts.get("harness_error").unwrap_or(&0);
        let supported = supported_match + supported_mismatch;
        let total_cases = cases.len();
        Self {
            total_cases,
            supported_match,
            supported_mismatch,
            skipped_unsupported,
            harness_error,
            supported_accuracy: (supported > 0)
                .then(|| supported_match as f64 / supported as f64),
            coverage: (total_cases > 0).then(|| supported as f64 / total_cases as f64),
        }
    }

    pub fn should_fail(&self) -> bool {
        self.supported_mismatch > 0 || self.harness_error > 0
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use crate::open_rules::discovery::discover_cases;

    use super::*;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn scores_match_mismatch_skip_and_harness_errors() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let candidate_root = repo_root().join("tests/fixtures/open_rules_candidate_reports");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");

        let scored = score_cases(&cases, &candidate_root);
        let summary = ScoreSummary::from_cases(&scored);

        assert_eq!(summary.total_cases, 6);
        assert_eq!(summary.supported_match, 2);
        assert_eq!(summary.supported_mismatch, 1);
        assert_eq!(summary.skipped_unsupported, 1);
        assert_eq!(summary.harness_error, 2);
        assert_eq!(summary.supported_accuracy, Some(2.0 / 3.0));
        assert_eq!(summary.coverage, Some(3.0 / 6.0));
        assert!(summary.should_fail());
    }

    #[test]
    fn candidate_report_path_mirrors_case_identity() {
        let open_rules_root = repo_root().join("tests/fixtures/open_rules_minimal");
        let cases = discover_cases(&open_rules_root, &[]).expect("discover cases");
        let case = cases
            .iter()
            .find(|case| case.rule_id == "CORE-000001" && case.case_kind.as_str() == "positive")
            .expect("positive case");

        assert_eq!(
            relative_candidate_report_path(case),
            Path::new("Published")
                .join("CORE-000001")
                .join("positive")
                .join("01")
                .join("report.csv")
        );
    }
}
```

- [ ] **Step 4: Run scoring tests**

Run:

```sh
cargo test -p xtask open_rules::score
```

Expected: PASS. The public `run` function remains a compiling stub in this task; Task 7 replaces it with the end-to-end command implementation.

- [ ] **Step 5: Commit scoring**

```sh
git add xtask/src/open_rules/score.rs
git commit -m "Score open rules oracle cases"
```

## Task 6: Implement Upstream Lock Handling

**Files:**
- Modify: `xtask/src/open_rules/upstream.rs`

- [ ] **Step 1: Write failing upstream tests**

Add tests to `xtask/src/open_rules/upstream.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_upstream_lock() {
        let dir = tempdir().expect("tempdir");
        let lock = dir.path().join("upstream.lock");
        fs::write(
            &lock,
            "repo=https://github.com/cdisc-org/cdisc-open-rules.git\nsha=7f7fae49376b3d023563ebb6c36a3b392d6e649f\n",
        )
        .expect("write lock");

        let parsed = read_upstream_lock(&lock).expect("read lock");

        assert_eq!(parsed.repo, "https://github.com/cdisc-org/cdisc-open-rules.git");
        assert_eq!(parsed.expected_sha.as_deref(), Some("7f7fae49376b3d023563ebb6c36a3b392d6e649f"));
    }

    #[test]
    fn missing_git_metadata_becomes_warning_not_error() {
        let dir = tempdir().expect("tempdir");
        let info = load_upstream_info_from_paths(dir.path(), dir.path().join("missing.lock").as_path())
            .expect("load upstream info");

        assert!(info.observed_sha.is_none());
        assert!(info.warnings.iter().any(|warning| warning.contains("git rev-parse")));
    }
}
```

- [ ] **Step 2: Run upstream tests to verify failure**

Run:

```sh
cargo test -p xtask open_rules::upstream
```

Expected: FAIL because upstream types and functions are not defined.

- [ ] **Step 3: Implement upstream handling**

Replace `xtask/src/open_rules/upstream.rs` with:

```rust
//! Upstream lock and checkout metadata for Open Rules scoring.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpstreamInfo {
    pub repo: String,
    pub expected_sha: Option<String>,
    pub observed_sha: Option<String>,
    pub lock_path: PathBuf,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamLock {
    pub repo: String,
    pub expected_sha: Option<String>,
}

pub fn load_upstream_info(open_rules_root: &Path) -> Result<UpstreamInfo> {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests/open_rules/upstream.lock");
    load_upstream_info_from_paths(open_rules_root, &lock_path)
}

pub fn load_upstream_info_from_paths(open_rules_root: &Path, lock_path: &Path) -> Result<UpstreamInfo> {
    let mut warnings = Vec::new();
    let lock = match read_upstream_lock(lock_path) {
        Ok(lock) => lock,
        Err(source) => {
            warnings.push(format!("could not read upstream lock {}: {source}", lock_path.display()));
            UpstreamLock {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: None,
            }
        }
    };
    let observed_sha = match observed_checkout_sha(open_rules_root) {
        Ok(sha) => Some(sha),
        Err(source) => {
            warnings.push(format!(
                "git rev-parse failed for {}: {source}",
                open_rules_root.display()
            ));
            None
        }
    };
    if let (Some(expected), Some(observed)) = (&lock.expected_sha, &observed_sha) {
        if expected != observed {
            warnings.push(format!(
                "upstream lock SHA {expected} does not match checkout SHA {observed}"
            ));
        }
    }

    Ok(UpstreamInfo {
        repo: lock.repo,
        expected_sha: lock.expected_sha,
        observed_sha,
        lock_path: lock_path.to_path_buf(),
        warnings,
    })
}

pub fn read_upstream_lock(path: &Path) -> Result<UpstreamLock> {
    let source = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut repo = None;
    let mut expected_sha = None;
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "repo" => repo = Some(value.trim().to_owned()),
            "sha" => expected_sha = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Ok(UpstreamLock {
        repo: repo.unwrap_or_else(|| "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned()),
        expected_sha,
    })
}

fn observed_checkout_sha(open_rules_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(open_rules_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .with_context(|| "run git rev-parse HEAD")?;
    if !output.status.success() {
        anyhow::bail!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_owned()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_upstream_lock() {
        let dir = tempdir().expect("tempdir");
        let lock = dir.path().join("upstream.lock");
        fs::write(
            &lock,
            "repo=https://github.com/cdisc-org/cdisc-open-rules.git\nsha=7f7fae49376b3d023563ebb6c36a3b392d6e649f\n",
        )
        .expect("write lock");

        let parsed = read_upstream_lock(&lock).expect("read lock");

        assert_eq!(parsed.repo, "https://github.com/cdisc-org/cdisc-open-rules.git");
        assert_eq!(parsed.expected_sha.as_deref(), Some("7f7fae49376b3d023563ebb6c36a3b392d6e649f"));
    }

    #[test]
    fn missing_git_metadata_becomes_warning_not_error() {
        let dir = tempdir().expect("tempdir");
        let info = load_upstream_info_from_paths(dir.path(), dir.path().join("missing.lock").as_path())
            .expect("load upstream info");

        assert!(info.observed_sha.is_none());
        assert!(info.warnings.iter().any(|warning| warning.contains("git rev-parse")));
    }
}
```

- [ ] **Step 4: Run upstream tests**

Run:

```sh
cargo test -p xtask open_rules::upstream
```

Expected: PASS.

- [ ] **Step 5: Commit upstream handling**

```sh
git add xtask/src/open_rules/upstream.rs
git commit -m "Read open rules upstream lock"
```

## Task 7: Implement Scoreboard Reports And Wire End-To-End Command

**Files:**
- Modify: `xtask/src/open_rules/report.rs`
- Modify: `xtask/src/open_rules/score.rs`

- [ ] **Step 1: Write failing report tests**

Add these tests to `xtask/src/open_rules/report.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::open_rules::score::{Scoreboard, ScoredCase, ScoreBucket, ScoreSummary};
    use crate::open_rules::upstream::UpstreamInfo;

    use super::*;

    #[test]
    fn writes_json_and_markdown_scoreboard() {
        let dir = tempdir().expect("tempdir");
        let scoreboard = Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("observed".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: vec!["warning text".to_owned()],
            },
            vec![ScoredCase {
                scope: "Published".to_owned(),
                rule_id: "CORE-000005".to_owned(),
                case_kind: "negative".to_owned(),
                case_id: "01".to_owned(),
                case_dir: "case".into(),
                official_results_csv: "official.csv".into(),
                candidate_report_csv: "report.csv".into(),
                bucket: ScoreBucket::SupportedMismatch,
                reason: None,
                official_issue_count: Some(1),
                candidate_issue_count: Some(1),
                missing: Vec::new(),
                extra: Vec::new(),
            }],
        );

        write_scoreboard(dir.path(), &scoreboard).expect("write scoreboard");

        let json = fs::read_to_string(dir.path().join("scoreboard.json")).expect("read json");
        let markdown = fs::read_to_string(dir.path().join("summary.md")).expect("read markdown");

        assert!(json.contains("\"supported_mismatch\": 1"));
        assert!(markdown.contains("# CDISC Open Rules Oracle Compatibility"));
        assert!(markdown.contains("CORE-000005"));
        assert!(markdown.contains("warning text"));
        assert!(scoreboard.summary.should_fail());
        assert_eq!(scoreboard.summary, ScoreSummary::from_cases(&scoreboard.cases));
    }
}
```

- [ ] **Step 2: Run report tests to verify failure**

Run:

```sh
cargo test -p xtask open_rules::report
```

Expected: FAIL because `Scoreboard` and `write_scoreboard` are not defined.

- [ ] **Step 3: Add scoreboard aggregation and end-to-end `run` wiring**

Edit `xtask/src/open_rules/score.rs` so the import section includes these items:

```rust
use crate::open_rules::discovery::{discover_cases, OpenRulesCase};
use crate::open_rules::normalize::{normalize_csv, IssueKey, ReportSource};
use crate::open_rules::report::write_scoreboard;
use crate::open_rules::upstream::{load_upstream_info, UpstreamInfo};
```

Replace the stub `run` function with:

```rust
pub fn run(args: ScoreArgs) -> anyhow::Result<bool> {
    let cases = discover_cases(&args.open_rules_root, &args.scope)?;
    let scored = score_cases(&cases, &args.core_rs_results_root);
    let upstream = load_upstream_info(&args.open_rules_root)?;
    let scoreboard = Scoreboard::new(upstream, scored);
    write_scoreboard(&args.out, &scoreboard)?;
    Ok(scoreboard.summary.should_fail())
}
```

Add these types after `ScoreSummary`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scoreboard {
    pub upstream: UpstreamInfo,
    pub summary: ScoreSummary,
    pub by_scope: Vec<GroupSummary>,
    pub by_case_kind: Vec<GroupSummary>,
    pub cases: Vec<ScoredCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupSummary {
    pub name: String,
    pub summary: ScoreSummary,
}
```

Add this implementation after `impl ScoreSummary`:

```rust
impl Scoreboard {
    pub fn new(upstream: UpstreamInfo, cases: Vec<ScoredCase>) -> Self {
        let summary = ScoreSummary::from_cases(&cases);
        let by_scope = grouped_summary(&cases, |case| case.scope.clone());
        let by_case_kind = grouped_summary(&cases, |case| case.case_kind.clone());
        Self {
            upstream,
            summary,
            by_scope,
            by_case_kind,
            cases,
        }
    }
}

fn grouped_summary(
    cases: &[ScoredCase],
    mut key: impl FnMut(&ScoredCase) -> String,
) -> Vec<GroupSummary> {
    let mut groups = BTreeMap::<String, Vec<ScoredCase>>::new();
    for case in cases {
        groups.entry(key(case)).or_default().push(case.clone());
    }
    groups
        .into_iter()
        .map(|(name, cases)| GroupSummary {
            name,
            summary: ScoreSummary::from_cases(&cases),
        })
        .collect()
}
```

- [ ] **Step 4: Implement report writing**

Replace `xtask/src/open_rules/report.rs` with:

```rust
//! Scoreboard JSON and Markdown report writing.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::open_rules::score::{ScoreBucket, Scoreboard};

pub fn write_scoreboard(out_dir: &Path, scoreboard: &Scoreboard) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let json_path = out_dir.join("scoreboard.json");
    let json_file = File::create(&json_path).with_context(|| format!("create {}", json_path.display()))?;
    serde_json::to_writer_pretty(json_file, scoreboard)
        .with_context(|| format!("write {}", json_path.display()))?;

    let markdown_path = out_dir.join("summary.md");
    let mut markdown = File::create(&markdown_path)
        .with_context(|| format!("create {}", markdown_path.display()))?;
    markdown
        .write_all(markdown_summary(scoreboard).as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))?;

    Ok(())
}

fn markdown_summary(scoreboard: &Scoreboard) -> String {
    let summary = &scoreboard.summary;
    let mut lines = vec![
        "# CDISC Open Rules Oracle Compatibility".to_owned(),
        String::new(),
        "| Metric | Value |".to_owned(),
        "|---|---:|".to_owned(),
        format!("| Total cases | {} |", summary.total_cases),
        format!("| Supported match | {} |", summary.supported_match),
        format!("| Supported mismatch | {} |", summary.supported_mismatch),
        format!("| Skipped unsupported | {} |", summary.skipped_unsupported),
        format!("| Harness error | {} |", summary.harness_error),
        format!("| Supported accuracy | {} |", percent_or_na(summary.supported_accuracy)),
        format!("| Coverage | {} |", percent_or_na(summary.coverage)),
        String::new(),
        "## Upstream".to_owned(),
        String::new(),
        format!("- Repo: `{}`", scoreboard.upstream.repo),
        format!(
            "- Expected SHA: `{}`",
            scoreboard
                .upstream
                .expected_sha
                .as_deref()
                .unwrap_or("not recorded")
        ),
        format!(
            "- Observed SHA: `{}`",
            scoreboard
                .upstream
                .observed_sha
                .as_deref()
                .unwrap_or("not available")
        ),
        String::new(),
    ];

    if !scoreboard.upstream.warnings.is_empty() {
        lines.push("## Warnings".to_owned());
        lines.push(String::new());
        for warning in &scoreboard.upstream.warnings {
            lines.push(format!("- {warning}"));
        }
        lines.push(String::new());
    }

    push_case_section(
        &mut lines,
        "Supported Mismatches",
        scoreboard,
        ScoreBucket::SupportedMismatch,
        50,
    );
    push_case_section(
        &mut lines,
        "Harness Errors",
        scoreboard,
        ScoreBucket::HarnessError,
        50,
    );
    push_case_section(
        &mut lines,
        "Skipped Unsupported Sample",
        scoreboard,
        ScoreBucket::SkippedUnsupported,
        10,
    );

    lines.join("\n") + "\n"
}

fn push_case_section(
    lines: &mut Vec<String>,
    title: &str,
    scoreboard: &Scoreboard,
    bucket: ScoreBucket,
    limit: usize,
) {
    let cases = scoreboard
        .cases
        .iter()
        .filter(|case| case.bucket == bucket)
        .take(limit)
        .collect::<Vec<_>>();
    if cases.is_empty() {
        return;
    }

    lines.push(format!("## {title}"));
    lines.push(String::new());
    for case in cases {
        let reason = case
            .reason
            .as_deref()
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default();
        lines.push(format!(
            "- `{}` {}/{}{} official={} candidate={}",
            case.rule_id,
            case.case_kind,
            case.case_id,
            reason,
            count_text(case.official_issue_count),
            count_text(case.candidate_issue_count)
        ));
    }
    lines.push(String::new());
}

fn count_text(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_owned())
}

fn percent_or_na(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.2}%", value * 100.0))
        .unwrap_or_else(|| "n/a".to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::open_rules::score::{Scoreboard, ScoredCase, ScoreBucket, ScoreSummary};
    use crate::open_rules::upstream::UpstreamInfo;

    use super::*;

    #[test]
    fn writes_json_and_markdown_scoreboard() {
        let dir = tempdir().expect("tempdir");
        let scoreboard = Scoreboard::new(
            UpstreamInfo {
                repo: "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned(),
                expected_sha: Some("expected".to_owned()),
                observed_sha: Some("observed".to_owned()),
                lock_path: "tests/open_rules/upstream.lock".into(),
                warnings: vec!["warning text".to_owned()],
            },
            vec![ScoredCase {
                scope: "Published".to_owned(),
                rule_id: "CORE-000005".to_owned(),
                case_kind: "negative".to_owned(),
                case_id: "01".to_owned(),
                case_dir: "case".into(),
                official_results_csv: "official.csv".into(),
                candidate_report_csv: "report.csv".into(),
                bucket: ScoreBucket::SupportedMismatch,
                reason: None,
                official_issue_count: Some(1),
                candidate_issue_count: Some(1),
                missing: Vec::new(),
                extra: Vec::new(),
            }],
        );

        write_scoreboard(dir.path(), &scoreboard).expect("write scoreboard");

        let json = fs::read_to_string(dir.path().join("scoreboard.json")).expect("read json");
        let markdown = fs::read_to_string(dir.path().join("summary.md")).expect("read markdown");

        assert!(json.contains("\"supported_mismatch\": 1"));
        assert!(markdown.contains("# CDISC Open Rules Oracle Compatibility"));
        assert!(markdown.contains("CORE-000005"));
        assert!(markdown.contains("warning text"));
        assert!(scoreboard.summary.should_fail());
        assert_eq!(scoreboard.summary, ScoreSummary::from_cases(&scoreboard.cases));
    }
}
```

- [ ] **Step 5: Run module tests together**

Run:

```sh
cargo test -p xtask open_rules
```

Expected: PASS.

- [ ] **Step 6: Run the command against synthetic fixtures**

Run:

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root tests/fixtures/open_rules_minimal \
  --core-rs-results-root tests/fixtures/open_rules_candidate_reports \
  --out target/open-rules-scoreboard-fixture
```

Expected: command exits non-zero because the fixture intentionally includes one mismatch and two harness errors. Files `target/open-rules-scoreboard-fixture/scoreboard.json` and `target/open-rules-scoreboard-fixture/summary.md` are written.

- [ ] **Step 7: Inspect fixture scoreboard**

Run:

```sh
rg "\"supported_match\": 2|\"supported_mismatch\": 1|\"skipped_unsupported\": 1|\"harness_error\": 2" target/open-rules-scoreboard-fixture/scoreboard.json
```

Expected: all four metric lines are found.

- [ ] **Step 8: Commit report writing and end-to-end wiring**

```sh
git add xtask/src/open_rules/report.rs xtask/src/open_rules/score.rs
git commit -m "Write open rules oracle scoreboard"
```

## Task 8: Add Documentation And Agent Guardrails

**Files:**
- Create: `docs/open-rules-oracle-harness.md`
- Create: `AGENTS.md`

- [ ] **Step 1: Create user-facing docs**

Create `docs/open-rules-oracle-harness.md`:

```markdown
# CDISC Open Rules Oracle Harness

`core-rust` treats `cdisc-org/cdisc-open-rules` as an oracle-backed
compatibility corpus. Each case is a combination of `rule.yml`, test data under
`data/`, and committed official `results/results.csv`.

Phase 1 is read-only. It scores existing core-rust `report.csv` files against
official `results.csv` files. It does not run core-rust, alter engine behavior,
apply baselines, or load `_variables.csv` as a schema authority.

## Command

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root /path/to/cdisc-open-rules \
  --core-rs-results-root target/open-rules-core-rs \
  --out target/open-rules-scoreboard
```

The default scope is `Published`. Add `--scope Unpublished` to include another
scope.

## Candidate Report Layout

Candidate reports must mirror the official case identity:

```text
<core-rs-results-root>/<scope>/<rule_id>/<case_kind>/<case_id>/report.csv
```

Example:

```text
target/open-rules-core-rs/Published/CORE-000001/negative/01/report.csv
```

## Buckets

| Bucket | Meaning | Command exit |
|---|---|---|
| `supported_match` | Candidate ran and normalized issue keys match the official oracle. | zero |
| `supported_mismatch` | Candidate ran but normalized issue keys differ. | non-zero |
| `skipped_unsupported` | Candidate output contains skipped rows. | zero |
| `harness_error` | Official or candidate report is missing, malformed, or cannot be scored. | non-zero |

Skipped and wrong are separate. Skipped cases are coverage gaps. Supported
mismatches are correctness problems.

## Metrics

```text
supported_accuracy = supported_match / (supported_match + supported_mismatch)
coverage = (supported_match + supported_mismatch) / total_cases
```

Coverage can be low while supported accuracy is high. That means the roadmap is
large, not that supported behavior is wrong.

## Normalization

The harness compares structural issue keys:

- rule id
- dataset
- domain
- row
- variables
- USUBJID
- sequence value

It does not compare diagnostic messages. Message text is retained in source
reports but is not a primary correctness key.

## Phase Roadmap

Phase 2 adds `_variables.csv` schema-aware CSV loading through a dedicated
Open Rules data path.

Phase 3 runs core-rust against selected cases and writes candidate reports into
the mirrored layout.

Phase 4 adds baseline policy, strict upstream lock enforcement, and CI.
```

- [ ] **Step 2: Create root `AGENTS.md`**

Create `AGENTS.md`:

```markdown
# Agent Guidance

## CDISC Open Rules Oracle Work

When working on `cdisc-open-rules` compatibility, treat
`cdisc-org/cdisc-open-rules` as an oracle-backed conformance corpus.

- Do not mix skipped and wrong. Skipped cases are coverage gaps; supported
  mismatches are correctness problems.
- Do not use diagnostic message text as a primary comparison key.
- Compare structural fields such as rule id, dataset/domain, row, variables,
  USUBJID, and sequence value.
- Keep Phase 1 read-only: discovery, normalization, scoring, and reports only.
- Do not change engine semantics in the same change as the Phase 1 harness.
- Keep `_variables.csv` type authority work in Phase 2.
- Use LLM-generated data only as a second layer after official oracle scoring is
  stable and official CORE output has confirmed expected results.
```

- [ ] **Step 3: Verify documentation references the command**

Run:

```sh
rg "open-rules score|supported_accuracy|skipped_unsupported|message" docs/open-rules-oracle-harness.md AGENTS.md
```

Expected: matches in both files.

- [ ] **Step 4: Commit docs and guardrails**

```sh
git add docs/open-rules-oracle-harness.md AGENTS.md
git commit -m "Document open rules oracle harness"
```

## Task 9: Final Verification

**Files:**
- Verify all files touched by Tasks 1-8.

- [ ] **Step 1: Format**

Run:

```sh
cargo fmt --all
```

Expected: command exits zero.

- [ ] **Step 2: Check workspace**

Run:

```sh
cargo check --workspace
```

Expected: command exits zero.

- [ ] **Step 3: Run tests**

Run:

```sh
cargo test --workspace
```

Expected: command exits zero.

- [ ] **Step 4: Run clippy**

Run:

```sh
cargo clippy --workspace -- -D warnings
```

Expected: command exits zero.

- [ ] **Step 5: Run fixture harness and confirm intentional non-zero**

Run:

```sh
cargo run -p xtask -- open-rules score \
  --open-rules-root tests/fixtures/open_rules_minimal \
  --core-rs-results-root tests/fixtures/open_rules_candidate_reports \
  --out target/open-rules-scoreboard-fixture
```

Expected: command exits non-zero because the fixture contains intentional
`supported_mismatch` and `harness_error` cases.

- [ ] **Step 6: Confirm fixture output exists**

Run:

```sh
test -f target/open-rules-scoreboard-fixture/scoreboard.json
test -f target/open-rules-scoreboard-fixture/summary.md
```

Expected: both commands exit zero.

- [ ] **Step 7: Review final diff**

Run:

```sh
git status --short
git diff --stat HEAD
```

Expected: only the intended harness, fixtures, docs, and guardrail files are modified or untracked. Existing unrelated files named `.gitignore 2`, `Cargo 2.lock`, `Cargo 2.toml`, and `README 2.md` remain untouched unless the user separately asks to handle them.

- [ ] **Step 8: Commit final verification fixes**

If formatting or warning fixes changed files, commit them:

```sh
git add Cargo.toml xtask tests/open_rules tests/fixtures/open_rules_minimal tests/fixtures/open_rules_candidate_reports docs/open-rules-oracle-harness.md AGENTS.md
git commit -m "Verify open rules oracle harness"
```

If Step 7 shows no remaining intended changes, skip this commit and report that all verification passed.
