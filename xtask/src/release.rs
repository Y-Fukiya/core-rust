use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Parser)]
pub struct ReleaseManifestArgs {
    #[arg(long, value_name = "FILE")]
    pub out: PathBuf,
    #[arg(long, value_name = "FILE")]
    pub artifact: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReleaseManifest {
    schema_version: u8,
    generated_by: String,
    package_name: String,
    package_version: String,
    rust_version: String,
    source_date_epoch: Option<String>,
    git: GitProvenance,
    artifacts: Vec<ReleaseArtifact>,
    verification_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReleaseArtifact {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GitProvenance {
    available: bool,
    commit: Option<String>,
    dirty: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseManifestInput {
    git_commit: Option<String>,
    git_dirty: Option<bool>,
    rust_version: String,
    source_date_epoch: Option<String>,
    artifacts: Vec<ReleaseArtifact>,
}

pub fn run(args: ReleaseManifestArgs) -> Result<bool> {
    let artifacts = args
        .artifact
        .iter()
        .map(|path| release_artifact(path.as_path()))
        .collect::<Result<Vec<_>>>()?;
    let manifest = build_release_manifest(ReleaseManifestInput {
        git_commit: git_output(["rev-parse", "HEAD"]),
        git_dirty: git_output(["status", "--porcelain"]).map(|status| !status.is_empty()),
        rust_version: command_output("rustc", ["--version"])
            .unwrap_or_else(|| "unknown".to_owned()),
        source_date_epoch: std::env::var("SOURCE_DATE_EPOCH").ok(),
        artifacts,
    });
    write_release_manifest(&args.out, &manifest)?;
    Ok(false)
}

fn build_release_manifest(input: ReleaseManifestInput) -> ReleaseManifest {
    let git_commit = input.git_commit.filter(|commit| !commit.is_empty());
    ReleaseManifest {
        schema_version: 1,
        generated_by: "xtask release-manifest".to_owned(),
        package_name: env!("CARGO_PKG_NAME").to_owned(),
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        rust_version: input.rust_version,
        source_date_epoch: input.source_date_epoch,
        git: GitProvenance {
            available: git_commit.is_some() && input.git_dirty.is_some(),
            commit: git_commit,
            dirty: input.git_dirty,
        },
        artifacts: input.artifacts,
        verification_commands: vec![
            "cargo fmt --all -- --check".to_owned(),
            "cargo check --workspace --locked".to_owned(),
            "cargo clippy --workspace --locked -- -D warnings".to_owned(),
            "cargo test --workspace --locked".to_owned(),
            "PYTHONPATH=src python3 -m pytest -q".to_owned(),
            "PYTHONPATH=src python3 scripts/p21port_smoke.py --work-dir <p21-workflow-out>"
                .to_owned(),
        ],
    }
}

fn release_artifact(path: &Path) -> Result<ReleaseArtifact> {
    Ok(ReleaseArtifact {
        path: path.display().to_string(),
        sha256: sha256_file(path)?,
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    if !path.is_file() {
        anyhow::bail!("artifact is not a file: {}", path.display());
    }
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .or_else(|_| Command::new("sha256sum").arg(path).output())
        .with_context(|| format!("sha256 {}", path.display()))?;
    if !output.status.success() {
        anyhow::bail!("sha256 failed for {}", path.display());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .filter(|hash| hash.len() == 64)
        .map(str::to_owned)
        .with_context(|| format!("parse sha256 output for {}", path.display()))
}

fn write_release_manifest(path: &Path, manifest: &ReleaseManifest) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    serde_json::to_writer_pretty(file, manifest)
        .with_context(|| format!("write {}", path.display()))
}

fn git_output<const N: usize>(args: [&str; N]) -> Option<String> {
    command_output("git", args)
}

fn command_output<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_manifest_records_git_version_and_verification_commands() {
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc 1.93.0".to_owned(),
            source_date_epoch: Some("1783123200".to_owned()),
            artifacts: Vec::new(),
        });

        assert_eq!(manifest.schema_version, 1);
        assert!(manifest.git.available);
        assert_eq!(manifest.git.commit.as_deref(), Some("abc123"));
        assert_eq!(manifest.git.dirty, Some(false));
        assert_eq!(manifest.rust_version, "rustc 1.93.0");
        assert_eq!(manifest.source_date_epoch.as_deref(), Some("1783123200"));
        assert!(manifest
            .verification_commands
            .contains(&"cargo test --workspace --locked".to_owned()));
    }

    #[test]
    fn release_manifest_includes_p21_workflow_smoke_commands() {
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc 1.93.0".to_owned(),
            source_date_epoch: None,
            artifacts: Vec::new(),
        });

        assert!(manifest
            .verification_commands
            .iter()
            .any(|command| command.contains("scripts/p21port_smoke.py")));
        assert!(!manifest
            .verification_commands
            .iter()
            .any(|command| command.contains("cdisc_rulekit.cli run-core")
                && command.contains("--dry-run")));
    }

    #[test]
    fn release_manifest_marks_git_provenance_unavailable_when_git_metadata_is_missing() {
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: None,
            git_dirty: None,
            rust_version: "rustc 1.93.0".to_owned(),
            source_date_epoch: None,
            artifacts: Vec::new(),
        });

        assert!(!manifest.git.available);
        assert_eq!(manifest.git.commit, None);
        assert_eq!(manifest.git.dirty, None);
    }

    #[test]
    fn release_manifest_writer_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("nested").join("release-manifest.json");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some(String::new()),
            git_dirty: Some(true),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            artifacts: Vec::new(),
        });

        write_release_manifest(&out, &manifest).expect("write manifest");

        let written = std::fs::read_to_string(out).expect("read manifest");
        assert!(written.contains("\"schema_version\": 1"));
        assert!(written.contains("\"dirty\": true"));
    }

    #[test]
    fn release_manifest_records_artifact_hashes() {
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc 1.93.0".to_owned(),
            source_date_epoch: None,
            artifacts: vec![ReleaseArtifact {
                path: "target/release/core-rs".to_owned(),
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_owned(),
            }],
        });

        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(manifest.artifacts[0].path, "target/release/core-rs");
        assert_eq!(
            manifest.artifacts[0].sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
