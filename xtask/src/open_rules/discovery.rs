//! Open Rules case discovery.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    Negative,
    Positive,
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
            anyhow::bail!("Open Rules scope does not exist: {}", scope_dir.display());
        }
        discover_scope(&scope, &scope_dir, &mut cases)?;
    }
    if cases.is_empty() {
        anyhow::bail!(
            "no Open Rules cases discovered under {}",
            open_rules_root.display()
        );
    }
    cases.sort_by(|left, right| {
        (&left.scope, &left.rule_id, left.case_kind, &left.case_id).cmp(&(
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
        assert_eq!(
            cases[0].env.get("PRODUCT").map(String::as_str),
            Some("SDTMIG")
        );
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
