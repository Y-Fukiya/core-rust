use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Parser)]
pub struct ReleaseManifestArgs {
    #[arg(long, value_name = "FILE")]
    pub out: PathBuf,
    #[arg(long, value_name = "FILE")]
    pub artifact: Vec<PathBuf>,
    #[arg(long, value_name = "DIR")]
    pub artifact_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Parser)]
pub struct ReleaseVerifyArgs {
    #[arg(long, value_name = "FILE")]
    pub manifest: PathBuf,
    #[arg(long, value_name = "DIR")]
    pub artifact_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReleaseManifest {
    schema_version: u8,
    generated_by: String,
    package_name: String,
    package_version: String,
    rust_version: String,
    source_date_epoch: Option<String>,
    cargo_lock_sha256: Option<String>,
    target_triple: Option<String>,
    ci_run_url: Option<String>,
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
    cargo_lock_sha256: Option<String>,
    target_triple: Option<String>,
    ci_run_url: Option<String>,
    artifacts: Vec<ReleaseArtifact>,
}

pub fn run(args: ReleaseManifestArgs) -> Result<bool> {
    let artifacts = args
        .artifact
        .iter()
        .map(|path| release_artifact(path.as_path(), args.artifact_root.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    let manifest = build_release_manifest(ReleaseManifestInput {
        git_commit: git_output(["rev-parse", "HEAD"]),
        git_dirty: git_output(["status", "--porcelain"]).map(|status| !status.is_empty()),
        rust_version: command_output("rustc", ["--version"])
            .unwrap_or_else(|| "unknown".to_owned()),
        source_date_epoch: std::env::var("SOURCE_DATE_EPOCH").ok(),
        cargo_lock_sha256: sha256_file(Path::new("Cargo.lock")).ok(),
        target_triple: rust_target_triple(),
        ci_run_url: github_actions_run_url(),
        artifacts,
    });
    write_release_manifest(&args.out, &manifest)?;
    Ok(false)
}

pub fn verify(args: ReleaseVerifyArgs) -> Result<bool> {
    verify_release_manifest(&args.manifest, args.artifact_root.as_deref())
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
        cargo_lock_sha256: input.cargo_lock_sha256,
        target_triple: input.target_triple,
        ci_run_url: input.ci_run_url,
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

fn release_artifact(path: &Path, artifact_root: Option<&Path>) -> Result<ReleaseArtifact> {
    Ok(ReleaseArtifact {
        path: release_artifact_path(path, artifact_root)?,
        sha256: sha256_file(path)?,
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    if !path.is_file() {
        anyhow::bail!("artifact is not a file: {}", path.display());
    }

    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn release_artifact_path(path: &Path, artifact_root: Option<&Path>) -> Result<String> {
    let Some(root) = artifact_root else {
        return Ok(path.display().to_string());
    };

    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))?;
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("canonicalize artifact root {}", root.display()))?;
    let relative = canonical_path
        .strip_prefix(&canonical_root)
        .with_context(|| {
            format!(
                "artifact {} is not under artifact root {}",
                path.display(),
                root.display()
            )
        })?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
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

fn verify_release_manifest(manifest_path: &Path, artifact_root: Option<&Path>) -> Result<bool> {
    let file = File::open(manifest_path)
        .with_context(|| format!("open release manifest {}", manifest_path.display()))?;
    let manifest: ReleaseManifest = serde_json::from_reader(file)
        .with_context(|| format!("parse release manifest {}", manifest_path.display()))?;
    let root = artifact_root.map(Path::to_path_buf).unwrap_or_else(|| {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    });
    let manifest_root = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut should_fail = false;

    if let Some(expected) = &manifest.cargo_lock_sha256 {
        let cargo_lock = manifest_root.join("Cargo.lock");
        match sha256_file(&cargo_lock) {
            Ok(actual) if actual == *expected => {}
            Ok(actual) => {
                eprintln!("Cargo.lock hash mismatch: expected {expected} actual {actual}");
                should_fail = true;
            }
            Err(error) => {
                eprintln!("Cargo.lock verification failed: {error:#}");
                should_fail = true;
            }
        }
    }

    for artifact in &manifest.artifacts {
        let path = root.join(&artifact.path);
        match sha256_file(&path) {
            Ok(actual) if actual == artifact.sha256 => {}
            Ok(actual) => {
                eprintln!(
                    "artifact hash mismatch: {} expected {} actual {}",
                    artifact.path, artifact.sha256, actual
                );
                should_fail = true;
            }
            Err(error) => {
                eprintln!("artifact verification failed: {}: {error:#}", artifact.path);
                should_fail = true;
            }
        }
    }

    Ok(should_fail)
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

fn rust_target_triple() -> Option<String> {
    command_output("rustc", ["-vV"]).and_then(|output| {
        output.lines().find_map(|line| {
            line.strip_prefix("host:")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
    })
}

fn github_actions_run_url() -> Option<String> {
    let server = std::env::var("GITHUB_SERVER_URL").ok()?;
    let repository = std::env::var("GITHUB_REPOSITORY").ok()?;
    let run_id = std::env::var("GITHUB_RUN_ID").ok()?;
    Some(format!("{server}/{repository}/actions/runs/{run_id}"))
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
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
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
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
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
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
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
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
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
            cargo_lock_sha256: Some("lockhash".to_owned()),
            target_triple: Some("aarch64-apple-darwin".to_owned()),
            ci_run_url: Some("https://github.example/run/1".to_owned()),
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
        assert_eq!(manifest.cargo_lock_sha256.as_deref(), Some("lockhash"));
        assert_eq!(
            manifest.target_triple.as_deref(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            manifest.ci_run_url.as_deref(),
            Some("https://github.example/run/1")
        );
    }

    #[test]
    fn release_artifact_records_root_relative_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = root.join("bin").join("core-rs");
        std::fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("mkdir");
        std::fs::write(&artifact, b"hello").expect("write artifact");

        let artifact = release_artifact(&artifact, Some(&root)).expect("artifact");

        assert_eq!(artifact.path, "bin/core-rs");
        assert_eq!(
            artifact.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn release_artifact_rejects_paths_outside_artifact_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = dir.path().join("outside.bin");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(&artifact, b"hello").expect("write artifact");

        let error = release_artifact(&artifact, Some(&root)).expect_err("outside root should fail");

        assert!(format!("{error:#}").contains("is not under artifact root"));
    }

    #[test]
    fn release_verify_accepts_matching_artifact_hashes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = root.join("bin").join("core-rs");
        std::fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("mkdir");
        std::fs::write(&artifact, b"hello").expect("write artifact");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![release_artifact(&artifact, Some(&root)).expect("artifact")],
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root)).expect("verify manifest");

        assert!(!should_fail);
    }

    #[test]
    fn release_verify_fails_when_artifact_hash_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = root.join("bin").join("core-rs");
        std::fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("mkdir");
        std::fs::write(&artifact, b"hello").expect("write artifact");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![release_artifact(&artifact, Some(&root)).expect("artifact")],
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");
        std::fs::write(&artifact, b"changed").expect("modify artifact");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root)).expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_fails_when_cargo_lock_hash_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let cargo_lock = dir.path().join("Cargo.lock");
        std::fs::write(&cargo_lock, b"lock-v1").expect("write lock");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: Some(sha256_file(&cargo_lock).expect("hash lock")),
            target_triple: None,
            ci_run_url: None,
            artifacts: Vec::new(),
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");
        std::fs::write(&cargo_lock, b"lock-v2").expect("modify lock");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root)).expect("verify manifest");

        assert!(should_fail);
    }
}
