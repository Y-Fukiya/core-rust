use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
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
    #[arg(long, value_name = "DIR")]
    pub source_root: Option<PathBuf>,
    #[arg(long, value_name = "TRIPLE")]
    pub target_triple: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct ReleaseVerifyArgs {
    #[arg(long, value_name = "FILE")]
    pub manifest: PathBuf,
    #[arg(long, value_name = "DIR")]
    pub artifact_root: Option<PathBuf>,
    #[arg(long, value_name = "DIR")]
    pub source_root: Option<PathBuf>,
    #[arg(long, value_name = "TRIPLE")]
    pub target_triple: Option<String>,
    #[arg(long)]
    pub require_clean_git: bool,
    #[arg(long)]
    pub require_ci_run_url: bool,
    #[arg(long)]
    pub require_source_date_epoch: bool,
    #[arg(long)]
    pub require_artifact: bool,
    #[arg(long)]
    pub require_cargo_lock: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReleaseVerifyPolicy {
    source_root: Option<PathBuf>,
    target_triple: Option<String>,
    require_clean_git: bool,
    require_ci_run_url: bool,
    require_source_date_epoch: bool,
    require_artifact: bool,
    require_cargo_lock: bool,
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
    ci: Option<CiProvenance>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CiProvenance {
    run_url: Option<String>,
    run_id: Option<String>,
    run_attempt: Option<String>,
    workflow: Option<String>,
    job: Option<String>,
    ref_name: Option<String>,
    ref_type: Option<String>,
    sha: Option<String>,
    actor: Option<String>,
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
    let source_root_is_explicit = args.source_root.is_some();
    let source_root = args
        .source_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let cargo_lock = source_root.join("Cargo.lock");
    let cargo_lock_sha256 = if source_root_is_explicit {
        Some(sha256_file(&cargo_lock).with_context(|| format!("hash {}", cargo_lock.display()))?)
    } else {
        sha256_file(&cargo_lock).ok()
    };
    let artifacts = args
        .artifact
        .iter()
        .map(|path| release_artifact(path.as_path(), args.artifact_root.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    let ci = github_actions_ci_provenance();
    let mut manifest = build_release_manifest(ReleaseManifestInput {
        git_commit: git_output(["rev-parse", "HEAD"]),
        git_dirty: git_output(["status", "--porcelain"]).map(|status| !status.is_empty()),
        rust_version: command_output("rustc", ["--version"])
            .unwrap_or_else(|| "unknown".to_owned()),
        source_date_epoch: std::env::var("SOURCE_DATE_EPOCH").ok(),
        cargo_lock_sha256,
        target_triple: args.target_triple.or_else(rust_target_triple),
        ci_run_url: ci.as_ref().and_then(|metadata| metadata.run_url.clone()),
        artifacts,
    });
    manifest.ci = ci.or_else(|| {
        manifest.ci_run_url.as_ref().map(|run_url| CiProvenance {
            run_url: Some(run_url.clone()),
            ..Default::default()
        })
    });
    write_release_manifest(&args.out, &manifest)?;
    Ok(false)
}

pub fn verify(args: ReleaseVerifyArgs) -> Result<bool> {
    verify_release_manifest(
        &args.manifest,
        args.artifact_root.as_deref(),
        ReleaseVerifyPolicy {
            source_root: args.source_root,
            target_triple: args.target_triple,
            require_clean_git: args.require_clean_git,
            require_ci_run_url: args.require_ci_run_url,
            require_source_date_epoch: args.require_source_date_epoch,
            require_artifact: args.require_artifact,
            require_cargo_lock: args.require_cargo_lock,
        },
    )
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
        ci_run_url: input.ci_run_url.clone(),
        ci: input.ci_run_url.map(|run_url| CiProvenance {
            run_url: Some(run_url),
            ..Default::default()
        }),
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
    if !canonical_root.is_dir() {
        anyhow::bail!("artifact root is not a directory: {}", root.display());
    }
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

fn verify_release_manifest(
    manifest_path: &Path,
    artifact_root: Option<&Path>,
    policy: ReleaseVerifyPolicy,
) -> Result<bool> {
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
    if !root.is_dir() {
        anyhow::bail!("artifact root is not a directory: {}", root.display());
    }
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("canonicalize artifact root {}", root.display()))?;
    let source_root = policy
        .source_root
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let mut should_fail = false;

    if manifest.schema_version != 1 {
        eprintln!(
            "unsupported release manifest schema_version: {}",
            manifest.schema_version
        );
        should_fail = true;
    }
    if policy.require_artifact && manifest.artifacts.is_empty() {
        eprintln!("at least one release artifact is required but missing from manifest");
        should_fail = true;
    }
    if policy.require_cargo_lock && manifest.cargo_lock_sha256.is_none() {
        eprintln!("Cargo.lock hash is required but missing from release manifest");
        should_fail = true;
    }

    if let Some(expected) = &policy.target_triple {
        match &manifest.target_triple {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                eprintln!("target triple mismatch: expected {expected} actual {actual}");
                should_fail = true;
            }
            None => {
                eprintln!("target triple is required but missing from release manifest");
                should_fail = true;
            }
        }
    }
    if policy.require_clean_git {
        if !manifest.git.available {
            eprintln!("clean git policy requires available git provenance");
            should_fail = true;
        }
        if manifest.git.dirty != Some(false) {
            eprintln!("clean git policy requires dirty=false");
            should_fail = true;
        }
    }
    if policy.require_ci_run_url && manifest.ci_run_url.is_none() {
        eprintln!("CI run URL is required but missing from release manifest");
        should_fail = true;
    }
    if policy.require_source_date_epoch && manifest.source_date_epoch.is_none() {
        eprintln!("SOURCE_DATE_EPOCH is required but missing from release manifest");
        should_fail = true;
    }

    if let Some(expected) = &manifest.cargo_lock_sha256 {
        let cargo_lock = source_root.join("Cargo.lock");
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
        let path = match verified_manifest_artifact_path(&canonical_root, &artifact.path) {
            Ok(path) => path,
            Err(error) => {
                eprintln!(
                    "artifact path verification failed: {}: {error:#}",
                    artifact.path
                );
                should_fail = true;
                continue;
            }
        };
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

fn verified_manifest_artifact_path(canonical_root: &Path, artifact_path: &str) -> Result<PathBuf> {
    let relative = Path::new(artifact_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        anyhow::bail!("artifact path must be relative and stay under artifact root");
    }
    let path = canonical_root.join(relative);
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("canonicalize artifact {}", path.display()))?;
    canonical_path
        .strip_prefix(canonical_root)
        .with_context(|| {
            format!(
                "artifact {} is not under artifact root {}",
                artifact_path,
                canonical_root.display()
            )
        })?;
    Ok(canonical_path)
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

fn github_actions_ci_provenance() -> Option<CiProvenance> {
    github_actions_ci_provenance_from_values(
        std::env::var("GITHUB_SERVER_URL").ok().as_deref(),
        std::env::var("GITHUB_REPOSITORY").ok().as_deref(),
        std::env::var("GITHUB_RUN_ID").ok().as_deref(),
        std::env::var("GITHUB_RUN_ATTEMPT").ok().as_deref(),
        std::env::var("GITHUB_WORKFLOW").ok().as_deref(),
        std::env::var("GITHUB_JOB").ok().as_deref(),
        std::env::var("GITHUB_REF_NAME").ok().as_deref(),
        std::env::var("GITHUB_REF_TYPE").ok().as_deref(),
        std::env::var("GITHUB_SHA").ok().as_deref(),
        std::env::var("GITHUB_ACTOR").ok().as_deref(),
    )
}

#[allow(clippy::too_many_arguments)]
fn github_actions_ci_provenance_from_values(
    server: Option<&str>,
    repository: Option<&str>,
    run_id: Option<&str>,
    run_attempt: Option<&str>,
    workflow: Option<&str>,
    job: Option<&str>,
    ref_name: Option<&str>,
    ref_type: Option<&str>,
    sha: Option<&str>,
    actor: Option<&str>,
) -> Option<CiProvenance> {
    let run_url = match (server, repository, run_id) {
        (Some(server), Some(repository), Some(run_id))
            if !server.is_empty() && !repository.is_empty() && !run_id.is_empty() =>
        {
            Some(format!("{server}/{repository}/actions/runs/{run_id}"))
        }
        _ => None,
    };
    let metadata = CiProvenance {
        run_url,
        run_id: non_empty(run_id),
        run_attempt: non_empty(run_attempt),
        workflow: non_empty(workflow),
        job: non_empty(job),
        ref_name: non_empty(ref_name),
        ref_type: non_empty(ref_type),
        sha: non_empty(sha),
        actor: non_empty(actor),
    };
    let has_metadata = metadata.run_url.is_some()
        || metadata.run_id.is_some()
        || metadata.run_attempt.is_some()
        || metadata.workflow.is_some()
        || metadata.job.is_some()
        || metadata.ref_name.is_some()
        || metadata.ref_type.is_some()
        || metadata.sha.is_some()
        || metadata.actor.is_some();
    has_metadata.then_some(metadata)
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
        assert_eq!(
            manifest.ci.as_ref().and_then(|ci| ci.run_url.as_deref()),
            Some("https://github.example/run/1")
        );
    }

    #[test]
    fn release_manifest_cli_accepts_target_triple_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = root.join("bin").join("core-rs");
        std::fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("mkdir");
        std::fs::write(&artifact, b"hello").expect("write artifact");
        let manifest_path = root.join("release-manifest.json");

        run(ReleaseManifestArgs {
            out: manifest_path.clone(),
            artifact: vec![artifact],
            artifact_root: Some(root.clone()),
            source_root: None,
            target_triple: Some("x86_64-unknown-linux-gnu".to_owned()),
        })
        .expect("write manifest");

        let file = File::open(&manifest_path).expect("open manifest");
        let manifest: ReleaseManifest = serde_json::from_reader(file).expect("parse manifest");

        assert_eq!(
            manifest.target_triple.as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(manifest.artifacts[0].path, "bin/core-rs");
    }

    #[test]
    fn release_manifest_cli_uses_source_root_for_cargo_lock_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&source_root).expect("mkdir source root");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(source_root.join("Cargo.lock"), b"lock-v1").expect("write lock");
        let manifest_path = root.join("release-manifest.json");

        run(ReleaseManifestArgs {
            out: manifest_path.clone(),
            artifact: Vec::new(),
            artifact_root: Some(root),
            source_root: Some(source_root.clone()),
            target_triple: None,
        })
        .expect("write manifest");

        let file = File::open(&manifest_path).expect("open manifest");
        let manifest: ReleaseManifest = serde_json::from_reader(file).expect("parse manifest");

        assert_eq!(
            manifest.cargo_lock_sha256.as_deref(),
            Some(
                sha256_file(&source_root.join("Cargo.lock"))
                    .expect("hash lock")
                    .as_str()
            )
        );
    }

    #[test]
    fn release_manifest_cli_fails_when_explicit_source_root_lacks_cargo_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&source_root).expect("mkdir source root");
        std::fs::create_dir_all(&root).expect("mkdir root");

        let error = run(ReleaseManifestArgs {
            out: root.join("release-manifest.json"),
            artifact: Vec::new(),
            artifact_root: Some(root),
            source_root: Some(source_root),
            target_triple: None,
        })
        .expect_err("explicit source root without Cargo.lock should fail");

        assert!(
            error.to_string().contains("hash") && format!("{error:#}").contains("Cargo.lock"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn release_manifest_records_structured_ci_metadata() {
        let ci = github_actions_ci_provenance_from_values(
            Some("https://github.com"),
            Some("Y-Fukiya/core-rust"),
            Some("123456"),
            Some("2"),
            Some("Release"),
            Some("build"),
            Some("main"),
            Some("branch"),
            Some("abc123"),
            Some("Y-Fukiya"),
        )
        .expect("ci metadata");

        assert_eq!(
            ci.run_url.as_deref(),
            Some("https://github.com/Y-Fukiya/core-rust/actions/runs/123456")
        );
        assert_eq!(ci.run_id.as_deref(), Some("123456"));
        assert_eq!(ci.run_attempt.as_deref(), Some("2"));
        assert_eq!(ci.workflow.as_deref(), Some("Release"));
        assert_eq!(ci.job.as_deref(), Some("build"));
        assert_eq!(ci.ref_name.as_deref(), Some("main"));
        assert_eq!(ci.ref_type.as_deref(), Some("branch"));
        assert_eq!(ci.sha.as_deref(), Some("abc123"));
        assert_eq!(ci.actor.as_deref(), Some("Y-Fukiya"));
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
    fn release_artifact_rejects_file_artifact_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle-file");
        let artifact = dir.path().join("artifact.bin");
        std::fs::write(&root, b"not a directory").expect("write root file");
        std::fs::write(&artifact, b"hello").expect("write artifact");

        let error = release_artifact(&artifact, Some(&root)).expect_err("file root should fail");

        assert!(format!("{error:#}").contains("artifact root is not a directory"));
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
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(!should_fail);
    }

    #[test]
    fn release_verify_rejects_manifest_artifact_paths_outside_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let outside = dir.path().join("outside.bin");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(&outside, b"hello").expect("write outside");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![ReleaseArtifact {
                path: "../outside.bin".to_owned(),
                sha256: sha256_file(&outside).expect("hash outside"),
            }],
        });
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_rejects_absolute_manifest_artifact_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let artifact = root.join("core-rs");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(&artifact, b"hello").expect("write artifact");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![ReleaseArtifact {
                path: artifact.display().to_string(),
                sha256: sha256_file(&artifact).expect("hash artifact"),
            }],
        });
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(should_fail);
    }

    #[cfg(unix)]
    #[test]
    fn release_verify_rejects_symlink_manifest_artifact_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let outside = dir.path().join("outside.bin");
        let link = root.join("link-to-outside");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(&outside, b"hello").expect("write outside");
        symlink(&outside, &link).expect("create symlink");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![ReleaseArtifact {
                path: "link-to-outside".to_owned(),
                sha256: sha256_file(&outside).expect("hash outside"),
            }],
        });
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(should_fail);
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
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_checks_each_recorded_artifact_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        let bin = root.join("bin").join("core-rs");
        let archive = root.join("archives").join("core-rs.tar.gz");
        std::fs::create_dir_all(bin.parent().expect("bin parent")).expect("mkdir bin parent");
        std::fs::create_dir_all(archive.parent().expect("archive parent"))
            .expect("mkdir archive parent");
        std::fs::write(&bin, b"binary").expect("write binary");
        std::fs::write(&archive, b"archive").expect("write archive");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: vec![
                release_artifact(&bin, Some(&root)).expect("binary artifact"),
                release_artifact(&archive, Some(&root)).expect("archive artifact"),
            ],
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify matching artifacts");
        assert!(!should_fail);

        std::fs::write(&archive, b"changed archive").expect("modify archive");
        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify changed artifacts");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_fails_when_cargo_lock_hash_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&source_root).expect("mkdir source root");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let cargo_lock = source_root.join("Cargo.lock");
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
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");
        std::fs::write(&cargo_lock, b"lock-v2").expect("modify lock");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                source_root: Some(source_root),
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_uses_source_root_for_cargo_lock_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&source_root).expect("mkdir source root");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let cargo_lock = source_root.join("Cargo.lock");
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
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                source_root: Some(source_root),
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(!should_fail);
    }

    #[test]
    fn release_verify_fails_when_target_triple_mismatches_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: Some("1783123200".to_owned()),
            cargo_lock_sha256: None,
            target_triple: Some("aarch64-apple-darwin".to_owned()),
            ci_run_url: Some("https://github.example/run/1".to_owned()),
            artifacts: Vec::new(),
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                target_triple: Some("x86_64-unknown-linux-gnu".to_owned()),
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_fails_unknown_schema_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let mut manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: Vec::new(),
        });
        manifest.schema_version = 2;
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail =
            verify_release_manifest(&manifest_path, Some(&root), ReleaseVerifyPolicy::default())
                .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_can_require_at_least_one_artifact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: Vec::new(),
        });
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                require_artifact: true,
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_can_require_cargo_lock_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(false),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: None,
            ci_run_url: None,
            artifacts: Vec::new(),
        });
        let manifest_path = root.join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                require_cargo_lock: true,
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(should_fail);
    }

    #[test]
    fn release_verify_enforces_ci_source_epoch_and_clean_git_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("bundle");
        std::fs::create_dir_all(&root).expect("mkdir root");
        let manifest = build_release_manifest(ReleaseManifestInput {
            git_commit: Some("abc123".to_owned()),
            git_dirty: Some(true),
            rust_version: "rustc test".to_owned(),
            source_date_epoch: None,
            cargo_lock_sha256: None,
            target_triple: Some("aarch64-apple-darwin".to_owned()),
            ci_run_url: None,
            artifacts: Vec::new(),
        });
        let manifest_path = dir.path().join("release-manifest.json");
        write_release_manifest(&manifest_path, &manifest).expect("write manifest");

        let should_fail = verify_release_manifest(
            &manifest_path,
            Some(&root),
            ReleaseVerifyPolicy {
                require_clean_git: true,
                require_ci_run_url: true,
                require_source_date_epoch: true,
                ..Default::default()
            },
        )
        .expect("verify manifest");

        assert!(should_fail);
    }
}
