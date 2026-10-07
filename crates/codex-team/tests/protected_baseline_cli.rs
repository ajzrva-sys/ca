use anyhow::{bail, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

struct Scratch {
    path: std::path::PathBuf,
}

impl Scratch {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn codex_team(repo: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(support::codex_team_binary())
        .args(args)
        .current_dir(repo)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()?)
}

fn git(repo: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("fixture Git command failed");
    }
    Ok(())
}

fn tree_canary(repo: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    for relative in ["oracles/data.bin", "reference/note.txt"] {
        hasher.update(relative.as_bytes());
        hasher.update(fs::read(repo.join(relative))?);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn fixture() -> Result<Scratch> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Scratch {
        path: std::env::temp_dir().join(format!(
            "codex-team-protected-cli-{}-{nonce}",
            std::process::id()
        )),
    };
    fs::create_dir(&root.path)?;
    fs::create_dir_all(root.path().join("oracles"))?;
    fs::create_dir_all(root.path().join("reference"))?;
    fs::write(root.path().join("oracles/data.bin"), [0_u8, 1, 2, 255])?;
    fs::write(root.path().join("reference/note.txt"), "evidence\n")?;
    fs::write(root.path().join(".gitignore"), "/oracles/\n/reference/\n")?;
    git(root.path(), &["init", "-q"])?;
    git(root.path(), &["config", "user.name", "Codex Team Test"])?;
    git(
        root.path(),
        &["config", "user.email", "codex-team@example.invalid"],
    )?;
    git(root.path(), &["add", ".gitignore"])?;
    git(root.path(), &["commit", "-q", "-m", "fixture"])?;
    Ok(root)
}

#[test]
fn protected_baseline_cli_is_digest_bound_head_anchored_and_mutation_free() -> Result<()> {
    let root = fixture()?;
    let before = tree_canary(root.path())?;
    let missing = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stdout).contains("baseline is missing"));

    let preview = codex_team(
        root.path(),
        &["protected-baseline", "preview", "--repo", ".", "--json"],
    )?;
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let receipt: Value = serde_json::from_slice(&preview.stdout)?;
    assert_eq!(
        receipt["schema"],
        "codex-team.protected_tree_baseline_preview.v1"
    );
    let revision = receipt["repository_revision"].as_str().unwrap();
    let digest = receipt["manifest_digest"].as_str().unwrap();

    let wrong = codex_team(
        root.path(),
        &[
            "protected-baseline",
            "apply",
            "--repo",
            ".",
            "--expected-revision",
            revision,
            "--accept-digest",
            &"0".repeat(64),
            "--json",
        ],
    )?;
    assert!(!wrong.status.success());
    assert!(!root
        .path()
        .join(".codex-team-protected-trees.json")
        .exists());

    let applied = codex_team(
        root.path(),
        &[
            "protected-baseline",
            "apply",
            "--repo",
            ".",
            "--expected-revision",
            revision,
            "--accept-digest",
            digest,
            "--json",
        ],
    )?;
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(tree_canary(root.path())?, before);

    let uncommitted = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(!uncommitted.status.success());
    assert!(
        String::from_utf8_lossy(&uncommitted.stdout).contains(".codex-team-protected-trees.json")
    );

    git(root.path(), &["add", ".codex-team-protected-trees.json"])?;
    git(root.path(), &["commit", "-q", "-m", "baseline"])?;
    let clean = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );

    fs::rename(
        root.path().join("reference/note.txt"),
        root.path().join("reference/renamed.txt"),
    )?;
    let drift = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(!drift.status.success());
    assert!(String::from_utf8_lossy(&drift.stdout).contains("baseline does not match"));
    Ok(())
}

#[test]
fn protected_baseline_cli_rejects_stale_revision_without_writing() -> Result<()> {
    let root = fixture()?;
    let preview = codex_team(
        root.path(),
        &["protected-baseline", "preview", "--repo", ".", "--json"],
    )?;
    let receipt: Value = serde_json::from_slice(&preview.stdout)?;
    let revision = receipt["repository_revision"].as_str().unwrap();
    let digest = receipt["manifest_digest"].as_str().unwrap();
    fs::write(root.path().join("tracked.txt"), "new revision\n")?;
    git(root.path(), &["add", "tracked.txt"])?;
    git(root.path(), &["commit", "-q", "-m", "advance"])?;

    let stale = codex_team(
        root.path(),
        &[
            "protected-baseline",
            "apply",
            "--repo",
            ".",
            "--expected-revision",
            revision,
            "--accept-digest",
            digest,
            "--json",
        ],
    )?;
    assert!(!stale.status.success());
    assert!(!root
        .path()
        .join(".codex-team-protected-trees.json")
        .exists());
    Ok(())
}

#[test]
fn protected_baseline_cli_excludes_only_exact_finder_metadata_names() -> Result<()> {
    let root = fixture()?;
    fs::create_dir_all(root.path().join("oracles/nested"))?;
    fs::write(root.path().join("oracles/.DS_Store"), b"initial")?;
    fs::write(
        root.path().join("oracles/nested/.DS_Store"),
        b"nested initial",
    )?;
    fs::write(root.path().join("reference/.DS_Store.keep"), b"protected")?;

    let preview = codex_team(
        root.path(),
        &["protected-baseline", "preview", "--repo", ".", "--json"],
    )?;
    assert!(preview.status.success());
    let receipt: Value = serde_json::from_slice(&preview.stdout)?;
    assert_eq!(
        receipt["manifest"]["algorithm"],
        "sha256-merkle-v2-ds-store-excluded"
    );
    let revision = receipt["repository_revision"].as_str().unwrap();
    let digest = receipt["manifest_digest"].as_str().unwrap();
    let applied = codex_team(
        root.path(),
        &[
            "protected-baseline",
            "apply",
            "--repo",
            ".",
            "--expected-revision",
            revision,
            "--accept-digest",
            digest,
            "--json",
        ],
    )?;
    assert!(applied.status.success());
    git(root.path(), &["add", ".codex-team-protected-trees.json"])?;
    git(root.path(), &["commit", "-q", "-m", "baseline"])?;

    fs::write(root.path().join("oracles/.DS_Store"), b"changed")?;
    fs::remove_file(root.path().join("oracles/nested/.DS_Store"))?;
    let ignored = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(
        ignored.status.success(),
        "{}",
        String::from_utf8_lossy(&ignored.stderr)
    );

    fs::write(root.path().join("reference/.DS_Store.keep"), b"drift")?;
    let protected = codex_team(
        root.path(),
        &["verify", "protected-trees", "--repo", ".", "--json"],
    )?;
    assert!(!protected.status.success());
    assert!(String::from_utf8_lossy(&protected.stdout).contains("baseline does not match"));
    Ok(())
}
