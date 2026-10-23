//! Private source snapshots for guidance experiments, including uncommitted
//! build inputs. This module never runs a Git mutation in the user's checkout.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{GuidanceSource, HarnessPlan, LearningVault};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
#[allow(dead_code)] // Shared build helper also exposes build-only entry points.
#[path = "../build_support/harness_identity.rs"]
pub(crate) mod identity;

fn collect(
    root: &Path,
    relative: &Path,
    files: &mut BTreeSet<PathBuf>,
    depth: usize,
    remaining: &mut usize,
    engineering: bool,
) -> Result<()> {
    crate::harness_idle::preparation_boundary()?;
    *remaining = remaining
        .checked_sub(1)
        .context("guidance inventory entry limit exceeded")?;
    if depth > 64 || files.len() >= 50_000 {
        bail!("guidance source inventory exceeds its bound");
    }
    let metadata = std::fs::symlink_metadata(root.join(relative))?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let name = entry.file_name();
            if engineering
                && matches!(
                    name.to_str(),
                    Some(".git" | "target" | "node_modules" | "oracles" | "reference")
                )
            {
                continue;
            }
            collect(
                root,
                &relative.join(name),
                files,
                depth + 1,
                remaining,
                engineering,
            )?;
        }
    } else if metadata.is_file() {
        if metadata.len() > 16 * 1024 * 1024 {
            bail!("guidance source file exceeds 16 MiB");
        }
        files.insert(relative.into());
    } else {
        bail!("guidance source contains a link or special file");
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    let mut remaining = 50_000;
    for input in identity::input_roots(root)? {
        collect(root, &input, &mut files, 0, &mut remaining, false)?;
    }
    // Engineering inputs beyond the binary build inventory. Do not copy oracle
    // trees, runtime state, credentials, Git history, or arbitrary root dotfiles.
    for name in [
        "tests", "crates", "docs", "scripts", "tools", ".github", ".agent",
    ] {
        let path = Path::new(name);
        if root.join(path).exists() {
            collect(root, path, &mut files, 0, &mut remaining, true)?;
        }
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("non-UTF-8 root source entry")?;
        if name.ends_with(".md")
            || matches!(
                name,
                ".gitignore"
                    | ".rustfmt.toml"
                    | "rustfmt.toml"
                    | "LICENSE"
                    | "Makefile"
                    | "justfile"
            )
        {
            collect(root, Path::new(name), &mut files, 0, &mut remaining, true)?;
        }
    }
    Ok(files)
}

fn bound_total(root: &Path, files: &BTreeSet<PathBuf>) -> Result<()> {
    let mut bytes = 0u64;
    for path in files {
        crate::harness_idle::preparation_boundary()?;
        bytes = bytes
            .checked_add(std::fs::symlink_metadata(root.join(path))?.len())
            .context("guidance source size overflow")?;
        if bytes > 512 * 1024 * 1024 {
            bail!("guidance source exceeds 512 MiB");
        }
    }
    Ok(())
}

pub(crate) fn source_digest(root: &Path) -> Result<String> {
    let roots = identity::input_roots(root)?;
    let mut files = BTreeSet::new();
    let mut remaining = 50_000;
    for input in &roots {
        collect(root, input, &mut files, 0, &mut remaining, false)?;
    }
    bound_total(root, &files)?;
    let mut remaining = 512u64 * 1024 * 1024;
    Ok(identity::digest_with_reader(
        files.into_iter().collect(),
        &Default::default(),
        &[],
        |relative| {
            crate::harness_idle::preparation_boundary().map_err(std::io::Error::other)?;
            let bytes = crate::workspace_materialization::read_bounded_regular_file(
                root,
                relative,
                remaining.min(16 * 1024 * 1024),
            )
            .map_err(std::io::Error::other)?;
            remaining -= bytes.len() as u64;
            Ok(bytes)
        },
    )?)
}

fn copy_inputs(origin: &Path, destination: &Path, files: &BTreeSet<PathBuf>) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    let mode = |path: &Path| -> Result<u32> { Ok(std::fs::symlink_metadata(path)?.mode() & 0o100) };
    let mut observed = BTreeMap::new();
    let mut remaining = 512u64 * 1024 * 1024;
    for path in files {
        crate::harness_idle::preparation_boundary()?;
        let executable = mode(&origin.join(path))?;
        let bytes = crate::workspace_materialization::read_bounded_regular_file(
            origin,
            path,
            remaining.min(16 * 1024 * 1024),
        )?;
        remaining = remaining
            .checked_sub(bytes.len() as u64)
            .context("guidance copy exceeds byte bound")?;
        observed.insert(
            path.clone(),
            (format!("{:x}", Sha256::digest(&bytes)), executable),
        );
        let output = destination.join(path);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(output.parent().context("missing snapshot parent")?)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600 | executable)
            .custom_flags(libc::O_NOFOLLOW)
            .open(output)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    if &inventory(origin)? != files {
        bail!("guidance inventory changed during copy");
    }
    for (path, (expected, executable)) in observed {
        crate::harness_idle::preparation_boundary()?;
        let bytes = crate::workspace_materialization::read_bounded_regular_file(
            origin,
            &path,
            16 * 1024 * 1024,
        )?;
        if format!("{:x}", Sha256::digest(bytes)) != expected
            || mode(&origin.join(path))? != executable
        {
            bail!("guidance source changed during copy");
        }
    }
    Ok(())
}

/// Runs only on the freezer's blocking worker, with a fixed host command and
/// kill-and-wait ownership. All commands operate inside a new private directory.
fn git(root: &Path, home: &Path, args: &[&str]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    struct Child(std::process::Child, bool, bool);
    impl Drop for Child {
        fn drop(&mut self) {
            if !self.1 {
                return;
            }
            if self.2 {
                unsafe {
                    libc::kill(-(self.0.id() as i32), libc::SIGKILL);
                }
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    crate::harness_idle::preparation_boundary()?;
    let mut child = Child(
        Command::new(ca_automation::git_metadata::EXECUTABLE)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
                "-c",
                "user.name=ca harness",
                "-c",
                "user.email=harness@localhost.invalid",
            ])
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(crate::jobs::owned_child_process_group())
            .spawn()?,
        true,
        !crate::jobs::worker_contained(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        crate::harness_idle::preparation_boundary()?;
        if let Some(status) = child.0.try_wait()? {
            child.1 = false;
            if !status.success() {
                bail!("private guidance snapshot Git operation failed");
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("private guidance snapshot Git operation timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn capture(
    vault: &LearningVault,
    original: &Path,
    expected: &str,
) -> Result<(GuidanceSource, tempfile::TempDir)> {
    use std::os::unix::fs::PermissionsExt;
    let origin = original.canonicalize()?;
    if source_digest(&origin)? != expected {
        bail!("source differs from the running build");
    }
    let files = inventory(&origin)?;
    let staging = tempfile::Builder::new()
        .prefix("guidance-source-")
        .tempdir_in(vault.resolve_relative("harness/inputs")?)?;
    std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))?;
    bound_total(&origin, &files)?;
    let repository = staging.path().canonicalize()?.join("repository");
    std::fs::create_dir(&repository)?;
    std::fs::set_permissions(&repository, std::fs::Permissions::from_mode(0o700))?;
    copy_inputs(&origin, &repository, &files)?;
    // Empty workspace-member directories are not represented by Git.
    std::fs::create_dir_all(repository.join("crates"))?;
    if source_digest(&repository)? != expected || source_digest(&origin)? != expected {
        bail!("source changed while freezing guidance");
    }
    let home = staging.path().join("home");
    std::fs::create_dir(&home)?;
    git(
        &repository,
        &home,
        &["init", "--quiet", "--initial-branch=harness-guidance"],
    )?;
    git(&repository, &home, &["add", "--force", "--all", "--", "."])?;
    git(
        &repository,
        &home,
        &[
            "commit",
            "--quiet",
            "-m",
            "Frozen guidance experiment inputs",
        ],
    )?;
    let result = GuidanceSource {
        origin,
        repository,
        source_digest: expected.into(),
        freezing_seconds: 0,
    };
    if source_digest(&result.repository)? != expected {
        bail!("snapshot source changed during Git initialization");
    }
    Ok((result, staging))
}

/// Check actual working bytes, not just the Git tree: checkout filters or index
/// flags must not make engineering verify a different source than the binary.
pub(crate) async fn verify_contents(plan: &HarnessPlan, root: &Path) -> Result<()> {
    let Some(source) = &plan.guidance_source else {
        return Ok(());
    };
    let expected = source.source_digest.clone();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        if source_digest(&root)? != expected {
            bail!("private guidance source differs from the frozen build inputs");
        }
        Ok(())
    })
    .await?
}

/// Resolve only a sealed snapshot under this vault. Original source may be dirty
/// or change later; it is provenance, never the engineering checkout to mutate.
pub(crate) fn resolve(
    vault: &LearningVault,
    plan: &HarnessPlan,
    requested: &Path,
) -> Result<PathBuf> {
    let requested = requested.canonicalize()?;
    let Some(snapshot) = &plan.guidance_source else {
        return Ok(requested);
    };
    let root = vault.resolve_relative("harness/inputs")?.canonicalize()?;
    let repository = snapshot.repository.canonicalize()?;
    if repository != snapshot.repository
        || !repository.starts_with(&root)
        || !repository
            .strip_prefix(&root)?
            .components()
            .next()
            .is_some_and(|part| {
                part.as_os_str()
                    .to_str()
                    .is_some_and(|name| name.starts_with("guidance-source-"))
            })
        || (requested != snapshot.origin && requested != repository)
    {
        bail!("guidance snapshot location changed or belongs to another repository");
    }
    Ok(repository)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn git_read(root: &Path, args: &[&str]) -> Vec<u8> {
        let output = std::process::Command::new(ca_automation::git_metadata::EXECUTABLE)
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }
    pub(crate) fn fixture(root: &Path) {
        crate::harness_engineering::tests::source(root);
        for (name, content) in [
            ("build.rs", "fn main() {}\n"),
            ("build_support/helper.rs", "// helper\n"),
            ("prompts/baseline.md", "baseline"),
            ("tests/fixtures/codex-model-catalog.json", "{}"),
            ("tests/reasoning_pilot_hidden/src/lib.rs", "// verifier\n"),
            ("src/untracked.rs", "// uncommitted source\n"),
            (".env", "SYNTHETIC_PRIVATE_SENTINEL"),
            ("oracles/never-copy.rs", "SYNTHETIC_ORACLE_SENTINEL"),
            ("reference/never-copy.rs", "SYNTHETIC_REFERENCE_SENTINEL"),
        ] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        std::fs::create_dir(root.join("crates")).unwrap();
        std::fs::write(root.join("crates/.keep"), "").unwrap();
        std::fs::write(
            root.join("src/bin/ca.rs"),
            "fn main() {\n    println!(\"dirty baseline\");\n}\n",
        )
        .unwrap();
    }
    #[test]
    fn hrs253_vm_authority_identity_survives_application_edits_but_rejects_control_edits() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fixture(&source);
        std::fs::write(source.join("src/harness_vm.rs"), "// protected adapter").unwrap();
        let authority = || {
            identity::digest(
                &source,
                &identity::authority_roots(
                    &source,
                    codex_team::self_improvement::HARNESS_PROTECTED_SOURCE,
                )
                .unwrap(),
                &Default::default(),
                &[],
            )
            .unwrap()
        };
        let initial = authority();
        std::fs::write(
            source.join("src/untracked.rs"),
            "// evaluated application repair",
        )
        .unwrap();
        assert_eq!(
            authority(),
            initial,
            "application repair must preserve recovery adapter identity"
        );
        for relative in [
            "src/harness_vm.rs",
            "build_support/helper.rs",
            "Cargo.toml",
            "Cargo.lock",
        ] {
            let path = source.join(relative);
            let before = std::fs::read(&path).unwrap_or_default();
            std::fs::write(&path, b"changed authority or dependency").unwrap();
            assert_ne!(
                authority(),
                initial,
                "{relative} must invalidate the adapter"
            );
            std::fs::write(&path, before).unwrap();
        }
    }

    #[test]
    fn hrs253_guidance_snapshot_preserves_dirty_source_index_and_excluded_inputs() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fixture(&source);
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let expected = source_digest(&source).unwrap();
        assert_eq!(
            expected,
            identity::digest(
                &source,
                &identity::input_roots(&source).unwrap(),
                &Default::default(),
                &[]
            )
            .unwrap()
        );
        let head = git_read(&source, &["rev-parse", "HEAD"]);
        let status = git_read(&source, &["status", "--porcelain", "--untracked-files=all"]);
        let index = std::fs::read(source.join(".git/index")).unwrap();
        assert!(!status.is_empty());
        let (mut snapshot, _owner) = capture(&vault, &source, &expected).unwrap();
        snapshot.freezing_seconds = 3;
        assert_eq!(source_digest(&snapshot.repository).unwrap(), expected);
        assert_eq!(
            std::fs::read(snapshot.repository.join("src/untracked.rs")).unwrap(),
            b"// uncommitted source\n"
        );
        for excluded in [".env", "oracles", "reference"] {
            assert!(!snapshot.repository.join(excluded).exists());
        }
        assert_eq!(git_read(&source, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            git_read(&source, &["status", "--porcelain", "--untracked-files=all"]),
            status
        );
        assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.baseline =
            codex_team::self_improvement::observe_clean_repository(&snapshot.repository).unwrap();
        plan.guidance_source = Some(snapshot.clone());
        plan.seal().unwrap();
        assert_eq!(
            codex_team::self_improvement::HarnessCycle::new(&plan, 0)
                .unwrap()
                .active_seconds,
            3
        );
        assert_eq!(
            resolve(&vault, &plan, &source).unwrap(),
            snapshot.repository
        );
        std::fs::write(source.join("src/untracked.rs"), "later user edit").unwrap();
        assert_eq!(source_digest(&snapshot.repository).unwrap(), expected);
        assert_eq!(
            resolve(&vault, &plan, &source).unwrap(),
            snapshot.repository
        );
        assert!(resolve(&vault, &plan, root.path()).is_err());
        plan.guidance_source.as_mut().unwrap().repository = source.canonicalize().unwrap();
        plan.seal().unwrap();
        assert!(resolve(&vault, &plan, &source).is_err());
    }
    #[test]
    fn hrs253_guidance_snapshot_rejects_source_drift_links_and_mixed_source_plans() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fixture(&source);
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        assert!(capture(&vault, &source, &"f".repeat(64)).is_err());
        std::os::unix::fs::symlink(root.path(), source.join("src/escape")).unwrap();
        assert!(source_digest(&source).is_err());
        std::fs::remove_file(source.join("src/escape")).unwrap();
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.guidance_source = Some(GuidanceSource {
            origin: source.canonicalize().unwrap(),
            repository: vault.root().into(),
            source_digest: "a".repeat(64),
            freezing_seconds: 1,
        });
        plan.source_files
            .insert("src/example.rs".into(), "a".repeat(64));
        assert!(plan.seal().is_err());
    }
}
