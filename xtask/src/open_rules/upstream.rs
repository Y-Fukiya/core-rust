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

pub fn load_upstream_info_from_paths(
    open_rules_root: &Path,
    lock_path: &Path,
) -> Result<UpstreamInfo> {
    let mut warnings = Vec::new();
    let lock = match read_upstream_lock(lock_path) {
        Ok(lock) => lock,
        Err(source) => {
            warnings.push(format!(
                "could not read upstream lock {}: {source}",
                lock_path.display()
            ));
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
        repo: repo
            .unwrap_or_else(|| "https://github.com/cdisc-org/cdisc-open-rules.git".to_owned()),
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

        assert_eq!(
            parsed.repo,
            "https://github.com/cdisc-org/cdisc-open-rules.git"
        );
        assert_eq!(
            parsed.expected_sha.as_deref(),
            Some("7f7fae49376b3d023563ebb6c36a3b392d6e649f")
        );
    }

    #[test]
    fn missing_git_metadata_becomes_warning_not_error() {
        let dir = tempdir().expect("tempdir");
        let info =
            load_upstream_info_from_paths(dir.path(), dir.path().join("missing.lock").as_path())
                .expect("load upstream info");

        assert!(info.observed_sha.is_none());
        assert!(info
            .warnings
            .iter()
            .any(|warning| warning.contains("git rev-parse")));
    }
}
