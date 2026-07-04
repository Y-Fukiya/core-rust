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
    verification_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GitProvenance {
    commit: Option<String>,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseManifestInput {
    git_commit: String,
    git_dirty: bool,
    rust_version: String,
    source_date_epoch: Option<String>,
}

pub fn run(args: ReleaseManifestArgs) -> Result<bool> {
    let manifest = build_release_manifest(ReleaseManifestInput {
        git_commit: git_output(["rev-parse", "HEAD"]).unwrap_or_default(),
        git_dirty: !git_output(["status", "--porcelain"])
            .unwrap_or_default()
            .is_empty(),
        rust_version: command_output("rustc", ["--version"])
            .unwrap_or_else(|| "unknown".to_owned()),
        source_date_epoch: std::env::var("SOURCE_DATE_EPOCH").ok(),
    });
    write_release_manifest(&args.out, &manifest)?;
    Ok(false)
}

fn build_release_manifest(input: ReleaseManifestInput) -> ReleaseManifest {
    ReleaseManifest {
        schema_version: 1,
        generated_by: "xtask release-manifest".to_owned(),
        package_name: env!("CARGO_PKG_NAME").to_owned(),
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        rust_version: input.rust_version,
        source_date_epoch: input.source_date_epoch,
        git: GitProvenance {
            commit: (!input.git_commit.is_empty()).then_some(input.git_commit),
            dirty: input.git_dirty,
        },
        verification_commands: vec![
            "cargo fmt --all -- --check".to_owned(),
            "cargo check --workspace --locked".to_owned(),
            "cargo clippy --workspace --locked -- -D warnings".to_owned(),
            "cargo test --workspace --locked".to_owned(),
            "PYTHONPATH=src python3 -m pytest -q".to_owned(),
        ],
    }
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
            git_commit: "abc123".to_owned(),
            git_dirty: false,
            rust_version: "rustc 1.93.0".to_owned(),
            source_date_epoch: Some("1783123200".to_owned()),
        });

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.git.commit.as_deref(), Some("abc123"));
        assert!(!manifest.git.dirty);
        assert_eq!(manifest.rust_version, "rustc 1.93.0");
        assert_eq!(manifest.source_date_epoch.as_deref(), Some("1783123200"));
        assert!(manifest
            .verification_commands
            .contains(&"cargo test --workspace --locked".to_owned()));
    }

    #[test]
    fn release_manifest_writer_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("nested").join("release-manifest.json");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: String::new(),
            git_dirty: true,
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
        });

        write_release_manifest(&out, &manifest).expect("write manifest");

        let written = std::fs::read_to_string(out).expect("read manifest");
        assert!(written.contains("\"schema_version\": 1"));
        assert!(written.contains("\"dirty\": true"));
    }
}
