//! Conservative compiler-derived Rust API impact baselines.

use super::{RustApiImpactStatusV1, RustApiImpactV1};
use crate::cancellation::CancellationToken;
use crate::paths::Paths;
use crate::revision::repository_revision;
use crate::verification::isolation::IsolationSession;
use crate::verification::recipe::{
    CachePolicy, CommandKind, FailurePolicy, Recipe, RecipeSpec, VerificationCommand,
};
use crate::verification::registry::{recipe_spec_to_command, validate_recipe_spec};
use crate::verification::runner::{
    run_isolated_structured_command_in_session_with_cancellation, DeterministicResultCategory,
};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_BASELINES: usize = 16;
const MAX_BASELINE_AGE: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone)]
struct Baseline {
    id: String,
    package: String,
    root: String,
    environment_fingerprint: String,
    rmeta_digest: Option<String>,
    public_digest: Option<String>,
    error: Option<String>,
    created: Instant,
}

#[derive(Debug)]
struct CompilerMetadata {
    rmeta_digest: String,
    public_digest: String,
}

static BASELINES: OnceLock<Mutex<VecDeque<Baseline>>> = OnceLock::new();

fn baselines() -> &'static Mutex<VecDeque<Baseline>> {
    BASELINES.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(crate) fn prime(repo: &Path, selected_paths: &[String]) -> Result<Vec<String>> {
    if selected_paths.is_empty() {
        return Ok(Vec::new());
    }
    let paths = Paths::new(repo)?;
    let revision = repository_revision(&paths)?;
    let packages = owning_rust_packages(repo, selected_paths)?;
    if packages.is_empty() || packages.len() > 2 {
        return Ok(Vec::new());
    }
    let isolation = IsolationSession::new(&paths)?;
    let environment_fingerprint =
        toolchain_fingerprint(&paths, &isolation, &CancellationToken::default())?;
    let mut created = Vec::new();
    let mut work = Vec::new();
    let mut store = baselines()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    store.retain(|entry| entry.created.elapsed() <= MAX_BASELINE_AGE);
    for (package, root) in &packages {
        let id = format!(
            "{:x}",
            Sha256::digest(
                format!("rust-api-v1\0{revision}\0{package}\0{root}\0{environment_fingerprint}")
                    .as_bytes()
            )
        );
        if !store.iter().any(|entry| entry.id == id) {
            store.push_front(Baseline {
                id: id.clone(),
                package: package.clone(),
                root: root.clone(),
                environment_fingerprint: environment_fingerprint.clone(),
                rmeta_digest: None,
                public_digest: None,
                error: None,
                created: Instant::now(),
            });
            work.push(((package.clone(), root.clone()), id.clone()));
        }
        created.push(id);
    }
    store.truncate(MAX_BASELINES);
    drop(store);
    if work.is_empty() {
        return Ok(created);
    }
    std::thread::Builder::new()
        .name("codex-team-rmeta-prime".to_string())
        .spawn(move || {
            let cancellation = CancellationToken::default();
            for ((package, root), baseline_id) in work {
                let result = compile_metadata(&paths, &isolation, &cancellation, &package, &root);
                let mut store = baselines()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                if let Some(entry) = store.iter_mut().find(|entry| entry.id == baseline_id) {
                    match result {
                        Ok(metadata) => {
                            entry.rmeta_digest = Some(metadata.rmeta_digest);
                            entry.public_digest = Some(metadata.public_digest);
                        }
                        Err(error) => entry.error = Some(bounded_reason(&error.to_string())),
                    }
                }
            }
        })
        .context("start Rust metadata baseline worker")?;
    Ok(created)
}

pub(crate) fn compare(
    repo: &Path,
    selected_paths: &[String],
    baseline_ids: &[String],
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> RustApiImpactV1 {
    match compare_inner(repo, selected_paths, baseline_ids, isolation, cancellation) {
        Ok(value) => value,
        Err(error) => RustApiImpactV1 {
            status: RustApiImpactStatusV1::Unknown,
            baseline_ids: baseline_ids.iter().take(2).cloned().collect(),
            packages: Vec::new(),
            before_metadata_digest: None,
            after_metadata_digest: None,
            reason_codes: vec![format!(
                "rust_metadata_unavailable:{}",
                bounded_reason(&error.to_string())
            )],
        },
    }
}

fn compare_inner(
    repo: &Path,
    selected_paths: &[String],
    baseline_ids: &[String],
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> Result<RustApiImpactV1> {
    if baseline_ids.is_empty() || baseline_ids.len() > 2 {
        bail!("no bounded Rust API baseline is available")
    }
    if selected_paths.iter().any(|path| {
        path == "Cargo.toml"
            || path == "Cargo.lock"
            || path.ends_with("/Cargo.toml")
            || path.ends_with("/build.rs")
    }) {
        bail!("Rust API proof is disabled for build or manifest-sensitive changes")
    }
    let paths = Paths::new(repo)?;
    let environment_fingerprint = toolchain_fingerprint(&paths, isolation, cancellation)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let selected = loop {
        let selected = {
            let store = baselines()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            baseline_ids
                .iter()
                .map(|id| {
                    store
                        .iter()
                        .find(|entry| &entry.id == id)
                        .cloned()
                        .with_context(|| format!("Rust API baseline `{id}` is missing or stale"))
                })
                .collect::<Result<Vec<_>>>()?
        };
        if let Some(error) = selected.iter().find_map(|entry| entry.error.as_deref()) {
            bail!("Rust API baseline failed: {error}")
        }
        if selected
            .iter()
            .all(|entry| entry.rmeta_digest.is_some() && entry.public_digest.is_some())
        {
            break selected;
        }
        if Instant::now() >= deadline {
            bail!("Rust API baseline was not ready within five seconds")
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if selected.iter().any(|entry| {
        entry.created.elapsed() > MAX_BASELINE_AGE
            || entry.environment_fingerprint != environment_fingerprint
            || !selected_paths
                .iter()
                .any(|path| path_in_root(path, &entry.root))
    }) {
        bail!("Rust API baseline identity does not match the current edit")
    }
    let mut unchanged = true;
    let mut packages = Vec::new();
    let mut ids = Vec::new();
    let mut before = Sha256::new();
    let mut after = Sha256::new();
    for baseline in selected {
        let current = compile_metadata(
            &paths,
            isolation,
            cancellation,
            &baseline.package,
            &baseline.root,
        )?;
        let baseline_rmeta = baseline
            .rmeta_digest
            .context("Rust API baseline .rmeta is unavailable")?;
        let baseline_public = baseline
            .public_digest
            .context("Rust public metadata baseline is unavailable")?;
        let package_unchanged =
            current.rmeta_digest == baseline_rmeta || current.public_digest == baseline_public;
        unchanged &= package_unchanged;
        before.update(baseline_public.as_bytes());
        after.update(current.public_digest.as_bytes());
        packages.push(baseline.package);
        ids.push(baseline.id);
    }
    packages.sort();
    packages.dedup();
    Ok(RustApiImpactV1 {
        status: if unchanged {
            RustApiImpactStatusV1::Unchanged
        } else {
            RustApiImpactStatusV1::Changed
        },
        baseline_ids: ids,
        packages,
        before_metadata_digest: Some(format!("{:x}", before.finalize())),
        after_metadata_digest: Some(format!("{:x}", after.finalize())),
        reason_codes: vec![if unchanged {
            "isolated_compiler_public_metadata_identical".to_string()
        } else {
            "isolated_rmeta_changed".to_string()
        }],
    })
}

fn compile_metadata(
    paths: &Paths,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
    package: &str,
    root: &str,
) -> Result<CompilerMetadata> {
    let spec = RecipeSpec {
        id: "mechanistic-rust-api-metadata".to_string(),
        recipe: Recipe::CargoRustcMetadata {
            package: package.to_string(),
        },
        cwd: (!root.is_empty()).then(|| root.to_string()),
        timeout_seconds: Some(300),
        kind: CommandKind::Build,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::Never,
        expected_exit: Some(0),
        source: "mechanistic.rust_api.v1".to_string(),
    };
    if !validate_recipe_spec(&spec) {
        bail!("Rust API metadata recipe is outside the typed registry");
    }
    let command = recipe_spec_to_command(spec);
    let result = run_isolated_structured_command_in_session_with_cancellation(
        paths,
        &command,
        isolation,
        cancellation,
    )?;
    if result.category != DeterministicResultCategory::Passed {
        bail!("isolated Rust metadata compilation failed");
    }
    let rmeta_digest = isolation.rust_metadata_digest(package)?;
    let public_spec = RecipeSpec {
        id: "mechanistic-rust-public-metadata".to_string(),
        recipe: Recipe::CargoRustdocPublic {
            package: package.to_string(),
        },
        cwd: (!root.is_empty()).then(|| root.to_string()),
        timeout_seconds: Some(300),
        kind: CommandKind::Build,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::Never,
        expected_exit: Some(0),
        source: "mechanistic.rust_api.v1".to_string(),
    };
    if !validate_recipe_spec(&public_spec) {
        bail!("Rust public metadata recipe is outside the typed registry");
    }
    let public_command = recipe_spec_to_command(public_spec);
    let public_result = run_isolated_structured_command_in_session_with_cancellation(
        paths,
        &public_command,
        isolation,
        cancellation,
    )?;
    if public_result.category != DeterministicResultCategory::Passed {
        bail!("isolated Rust public metadata compilation failed");
    }
    Ok(CompilerMetadata {
        rmeta_digest,
        public_digest: isolation.rust_public_api_digest(package)?,
    })
}

fn owning_rust_packages(repo: &Path, selected_paths: &[String]) -> Result<Vec<(String, String)>> {
    let tracked = super::query::tracked_files(repo)?;
    let mut packages = Vec::new();
    for manifest in tracked.iter().filter(|path| path.ends_with("Cargo.toml")) {
        let bytes = fs::read(repo.join(manifest))?;
        if bytes.len() > 1024 * 1024 {
            bail!("tracked Cargo manifest exceeds 1 MiB")
        }
        let value: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)?;
        let Some(name) = value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
        else {
            continue;
        };
        let root = Path::new(manifest)
            .parent()
            .and_then(Path::to_str)
            .unwrap_or_default()
            .to_string();
        packages.push((name.to_string(), root));
    }
    let mut owners = BTreeSet::new();
    for path in selected_paths.iter().filter(|path| path.ends_with(".rs")) {
        if let Some((name, root)) = packages
            .iter()
            .filter(|(_, root)| path_in_root(path, root))
            .max_by_key(|(_, root)| root.len())
        {
            owners.insert((name.clone(), root.clone()));
        }
    }
    Ok(owners.into_iter().take(3).collect())
}

fn path_in_root(path: &str, root: &str) -> bool {
    root.is_empty()
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn toolchain_fingerprint(
    paths: &Paths,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> Result<String> {
    let mut command = VerificationCommand::new(
        "rustc".into(),
        vec!["-vV".into()],
        "mechanistic.rust_api.v1",
    );
    command.timeout_seconds = Some(15);
    command.expected_exit = Some(0);
    let output = run_isolated_structured_command_in_session_with_cancellation(
        paths,
        &command,
        isolation,
        cancellation,
    )
    .context("inspect isolated Rust toolchain for API metadata")?;
    if output.category != DeterministicResultCategory::Passed
        || output.stdout_tail.len() > 64 * 1024
    {
        bail!("Rust toolchain fingerprint is unavailable")
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(output.stdout_tail.as_bytes())
    ))
}

fn bounded_reason(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn run(repo: &Path, program: &str, args: &[&str]) -> Result<()> {
        let output = Command::new(program)
            .args(args)
            .current_dir(repo)
            .output()?;
        if !output.status.success() {
            bail!(
                "fixture command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn fixture() -> Result<PathBuf> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let repo = PathBuf::from("/tmp").join(format!(
            "codex-team-rust-api-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(repo.join("src"))?;
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"broker-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        fs::write(
            repo.join("src/lib.rs"),
            "pub fn public_api(value: u8) -> u8 { private(value) }\nfn private(value: u8) -> u8 { value.saturating_add(1) }\n",
        )?;
        run(&repo, "git", &["init", "--quiet"])?;
        run(&repo, "cargo", &["generate-lockfile", "--offline"])?;
        run(
            &repo,
            "git",
            &["add", "Cargo.toml", "Cargo.lock", "src/lib.rs"],
        )?;
        run(
            &repo,
            "git",
            &[
                "-c",
                "user.name=codex-team-test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ],
        )?;
        Ok(repo)
    }

    fn wait_for_baseline(id: &str) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let outcome = {
                let store = baselines()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                store.iter().find(|entry| entry.id == id).map(|entry| {
                    (
                        entry.rmeta_digest.is_some() && entry.public_digest.is_some(),
                        entry.error.clone(),
                    )
                })
            };
            match outcome {
                Some((true, _)) => return Ok(()),
                Some((false, Some(error))) => bail!("baseline failed: {error}"),
                Some((false, None)) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => bail!("baseline did not finish within twenty seconds"),
            }
        }
    }

    #[test]
    fn private_body_keeps_rmeta_while_public_signature_changes_it() -> Result<()> {
        crate::verification::isolation::enable_test_isolation_backend();
        let repo = fixture()?;
        let selected = vec!["src/lib.rs".to_string()];
        let ids = prime(&repo, &selected)?;
        assert_eq!(ids.len(), 1);
        wait_for_baseline(&ids[0])?;
        assert_eq!(prime(&repo, &selected)?, ids);
        assert_eq!(
            baselines()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .iter()
                .filter(|entry| entry.id == ids[0])
                .count(),
            1
        );

        fs::write(
            repo.join("src/lib.rs"),
            "pub fn public_api(value: u8) -> u8 { private(value) }\nfn private(value: u8) -> u8 { value.saturating_add(2) }\n",
        )?;
        let paths = Paths::new(&repo)?;
        let isolation = IsolationSession::new(&paths)?;
        let private = compare(
            &repo,
            &selected,
            &ids,
            &isolation,
            &CancellationToken::default(),
        );
        assert_eq!(
            private.status,
            RustApiImpactStatusV1::Unchanged,
            "{private:?}"
        );

        fs::write(
            repo.join("src/lib.rs"),
            "pub fn public_api(value: u16) -> u16 { private(value) }\nfn private(value: u16) -> u16 { value.saturating_add(2) }\n",
        )?;
        let isolation = IsolationSession::new(&Paths::new(&repo)?)?;
        let public = compare(
            &repo,
            &selected,
            &ids,
            &isolation,
            &CancellationToken::default(),
        );
        assert_eq!(public.status, RustApiImpactStatusV1::Changed, "{public:?}");
        fs::remove_dir_all(repo)?;
        Ok(())
    }
}
