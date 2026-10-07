//! Private source commits. The model supplies no Git commands, and this stage
//! cannot modify the original checkout or grant candidate/engineering approval.
use crate::harness_activity::HarnessActivity;
use crate::harness_engineering::{execute_capture, fixed_git_command, source_snapshot};
use crate::harness_source_generation::{load, SourceGeneration};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, load_harness_cycle, update_harness_cycle, HarnessStage, LearningVault,
    SourceRepositorySnapshotV1,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCheckout {
    pub plan_digest: String,
    pub generation_digest: String,
    pub repository: PathBuf,
    pub workspace: PathBuf,
    pub started: bool,
    pub candidate: Option<SourceRepositorySnapshotV1>,
}

async fn git(root: &Path, temporary: &Path, args: &[&std::ffi::OsStr]) -> Result<String> {
    let mut command = fixed_git_command(root, temporary).await?;
    command.args([
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
    ]);
    if args
        .first()
        .is_some_and(|arg| *arg == std::ffi::OsStr::new("diff"))
    {
        command
            .args(["diff", "--no-ext-diff", "--no-textconv"])
            .args(&args[1..]);
    } else {
        command.args(args);
    }
    let output = execute_capture(command, Duration::from_secs(45)).await?;
    if !output.passed {
        bail!("private source Git operation failed");
    }
    // All callers expect a small identity/list or discard ordinary command output.
    if output.out.1.len() >= 8192 {
        bail!("private source Git output exceeded bound");
    }
    Ok(output.out.1)
}
fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
    values.iter().map(Into::into).collect()
}

/// Import only the verified commit's objects. No local ref, index, working file,
/// FETCH_HEAD or installed executable is changed by this preparation step.
pub async fn import_candidate(vault: LearningVault, id: &str, repository: &Path) -> Result<()> {
    // Keep the combined audit/process state off callers' executor stacks.
    Box::pin(import_candidate_inner(vault, id, repository)).await
}

async fn import_candidate_inner(vault: LearningVault, id: &str, repository: &Path) -> Result<()> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, cycle) = load_harness_cycle(&vault, id)?;
    if cycle.stage != HarnessStage::ReadyToPromote
        || !cycle.reservations.is_empty()
        || !cycle.active_reservations.is_empty()
        || plan.source_files.is_empty()
    {
        bail!("source import requires a settled source promotion decision");
    }
    let (_, harness, artifact) = crate::harness_source_build::load_candidate(&vault, id)?;
    let candidate_digest = harness.digest()?;
    let decision = codex_team::self_improvement::evaluate_harness(
        &plan,
        &candidate_digest,
        &cycle.trials,
        cycle.engineering_checks_passed,
    )?;
    if decision.verdict != codex_team::self_improvement::HarnessVerdict::Promote {
        bail!("frozen trial receipts do not authorize source import");
    }
    let checkout: SourceCheckout = vault
        .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
        .context("source checkout missing")?;
    let saved = load(&vault, id)?.context("source generation missing")?;
    let canonical_repository = repository.canonicalize()?;
    let private_root = vault
        .resolve_relative("harness/source-checkouts")?
        .canonicalize()?;
    if checkout.repository != canonical_repository
        || !checkout
            .workspace
            .canonicalize()?
            .starts_with(&private_root)
        || checkout.workspace.canonicalize()? == private_root
    {
        bail!("source import repository identity changed");
    }
    let receipt = serde_json::json!({
        "schema": "ca.harness_source_import.v1",
        "plan_digest": plan.digest,
        "checkout_digest": canonical_digest(&checkout)?,
        "candidate_digest": candidate_digest,
        "trials_digest": decision.trials_digest,
        "repository": canonical_repository,
    });
    let receipt_path = format!("harness/source/{id}-import.json");
    let previous = vault.read_mutable::<serde_json::Value>(&receipt_path)?;
    if previous
        .as_ref()
        .is_some_and(|previous| previous != &receipt)
    {
        bail!("source import receipt binding changed");
    }
    let maximum = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(cycle.active_seconds)
        .min(120);
    let activity = HarnessActivity::begin(vault.clone(), plan.clone(), maximum).await?;
    activity
        .run(async {
            let temporary = tempfile::tempdir()?;
            audit(&checkout, &saved, &plan, temporary.path()).await?;
            let expected_binary = harness.candidate_binary_digest.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                if crate::harness_engineering::artifact_digest(&artifact)? != expected_binary {
                    bail!("candidate executable changed before source import");
                }
                Ok(())
            })
            .await??;
            let commit = harness
                .candidate_commit
                .as_deref()
                .context("candidate commit missing")?;
            if previous.is_none() {
                let mut command = args(&[
                    "-c",
                    "protocol.allow=never",
                    "-c",
                    "protocol.file.allow=always",
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    "--no-auto-maintenance",
                    "--no-recurse-submodules",
                    "--",
                ]);
                command.push(checkout.workspace.as_os_str().to_owned());
                command.push(commit.into());
                run_git(repository, temporary.path(), command).await?;
            }
            let imported = run_git(
                repository,
                temporary.path(),
                args(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")]),
            )
            .await?;
            if imported.trim() != commit {
                bail!("source import resolved a different commit");
            }
            audit(&checkout, &saved, &plan, temporary.path()).await?;
            vault.write_mutable(&receipt_path, &receipt)?;
            Ok(())
        })
        .await
}
pub(crate) async fn run_git(
    root: &Path,
    temporary: &Path,
    values: Vec<std::ffi::OsString>,
) -> Result<String> {
    git(
        root,
        temporary,
        &values
            .iter()
            .map(|value| value.as_os_str())
            .collect::<Vec<_>>(),
    )
    .await
}

pub(crate) async fn audit(
    journal: &SourceCheckout,
    saved: &SourceGeneration,
    plan: &codex_team::self_improvement::HarnessPlan,
    temporary: &Path,
) -> Result<()> {
    if source_snapshot(&journal.repository).await? != plan.baseline {
        bail!("original source changed");
    }
    audit_candidate(journal, saved, plan, temporary).await
}

pub(crate) async fn audit_candidate(
    journal: &SourceCheckout,
    saved: &SourceGeneration,
    plan: &codex_team::self_improvement::HarnessPlan,
    temporary: &Path,
) -> Result<()> {
    let baseline = &plan.baseline;
    let replacements = saved
        .proposal
        .as_ref()
        .context("source proposal missing")?
        .apply(&saved.inputs, plan)?;
    let root = journal.workspace.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for (path, expected) in replacements {
            let actual = crate::workspace_materialization::read_bounded_regular_file(
                &root,
                Path::new(&path),
                256 * 1024,
            )?;
            if actual != expected.as_bytes() {
                bail!("candidate bytes differ from the frozen proposal");
            }
        }
        Ok(())
    })
    .await??;
    let expected = journal
        .candidate
        .as_ref()
        .context("source commit missing")?;
    if source_snapshot(&journal.workspace).await? != *expected {
        bail!("candidate source changed");
    }
    let parent = run_git(&journal.workspace, temporary, args(&["rev-parse", "HEAD^"])).await?;
    if parent.trim() != baseline.head {
        bail!("candidate is not exactly one commit above its baseline");
    }
    let changed = run_git(
        &journal.workspace,
        temporary,
        args(&[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            &baseline.head,
            &expected.head,
        ]),
    )
    .await?;
    let proposal = saved.proposal.as_ref().context("source proposal missing")?;
    let expected_paths = proposal
        .edits
        .iter()
        .map(|edit| edit.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let paths = changed
        .split('\0')
        .filter(|path| !path.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if paths.is_empty() || !paths.is_subset(&expected_paths) {
        bail!("candidate changed unproposed paths");
    }
    run_git(
        &journal.workspace,
        temporary,
        args(&["diff", "--check", &baseline.head, &expected.head]),
    )
    .await?;
    Ok(())
}

pub fn report(vault: &LearningVault, id: &str) -> Result<Option<serde_json::Value>> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    let journal: Option<SourceCheckout> =
        vault.read_mutable(&format!("harness/source-checkouts/{id}.json"))?;
    let Some(journal) = journal else {
        return Ok(None);
    };
    let saved = load(vault, id)?.context("source checkout has no generation")?;
    if journal.plan_digest != plan.digest || journal.generation_digest != canonical_digest(&saved)?
    {
        bail!("source checkout journal binding changed");
    }
    let imported =
        vault.read_mutable::<serde_json::Value>(&format!("harness/source/{id}-import.json"))?;
    if let Some(receipt) = &imported {
        if receipt["schema"] != "ca.harness_source_import.v1"
            || receipt["plan_digest"] != plan.digest
            || receipt["checkout_digest"] != canonical_digest(&journal)?
            || receipt["repository"] != serde_json::to_value(&journal.repository)?
        {
            bail!("source import receipt identity changed");
        }
    }
    Ok(Some(
        serde_json::json!({"started":journal.started,"candidate":journal.candidate,
        "generation_digest":journal.generation_digest,"import":imported}),
    ))
}

pub async fn prepare(vault: LearningVault, id: &str, repository: &Path) -> Result<SourceCheckout> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, state) = load_harness_cycle(&vault, id)?;
    let saved = load(&vault, id)?.context("no source generation")?;
    let proposed = saved
        .proposal
        .as_ref()
        .context("source generation incomplete")?
        .apply(&saved.inputs, &plan)?;
    if state.stage != HarnessStage::EvaluationFrozen
        || state.candidate_digest.is_some()
        || !state.reservations.is_empty()
        || !state.active_reservations.is_empty()
    {
        bail!("source checkout requires a settled, frozen cycle");
    }
    let path = format!("harness/source-checkouts/{id}.json");
    let repository = repository.canonicalize()?;
    let generation_digest = canonical_digest(&saved)?;
    let existing: Option<SourceCheckout> = vault.read_mutable(&path)?;
    if let Some(journal) = &existing {
        if journal.plan_digest != plan.digest
            || journal.generation_digest != generation_digest
            || journal.repository != repository
        {
            bail!("source checkout journal binding changed");
        }
        if journal.started || journal.candidate.is_none() {
            update_harness_cycle(&vault, id, |state| {
                state.stage = HarnessStage::RecoveryRequired;
                state.reason = "Interrupted source checkout; no automatic replay".into();
                Ok(())
            })?;
            bail!("source checkout requires recovery");
        }
    }
    let seconds = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(state.active_seconds)
        .min(180);
    let activity = HarnessActivity::begin(vault.clone(), plan.clone(), seconds).await?;
    let root = vault.resolve_relative("harness/source-checkouts")?;
    std::fs::create_dir_all(&root)?;
    let temporary = tempfile::tempdir()?;
    std::fs::create_dir(temporary.path().join("home"))?;
    let result = activity
        .run(async {
            if let Some(journal) = existing {
                let workspace = journal.workspace.canonicalize()?;
                if !workspace.starts_with(root.canonicalize()?) {
                    bail!("source checkout escaped private root");
                }
                audit(&journal, &saved, &plan, temporary.path()).await?;
                return Ok(journal);
            }
            if source_snapshot(&repository).await? != plan.baseline {
                bail!("source baseline changed before checkout");
            }
            let directory = tempfile::Builder::new()
                .prefix("candidate-")
                .tempdir_in(&root)?
                .keep();
            let workspace = directory.join("source");
            let mut journal = SourceCheckout {
                plan_digest: plan.digest.clone(),
                generation_digest,
                repository: repository.clone(),
                workspace: workspace.clone(),
                started: true,
                candidate: None,
            };
            {
                let _lock = vault.acquire_lock()?;
                vault.write_mutable(&path, &journal)?;
            }
            let mut command = args(&["clone", "--local", "--no-hardlinks", "--quiet", "--"]);
            command.push(repository.into_os_string());
            command.push(workspace.clone().into_os_string());
            run_git(&directory, temporary.path(), command).await?;
            if source_snapshot(&workspace).await? != plan.baseline {
                bail!("private clone differs from frozen source");
            }
            let branch = format!("harness-{}", plan.id);
            run_git(
                &workspace,
                temporary.path(),
                args(&["checkout", "--quiet", "-b", &branch]),
            )
            .await?;
            let before = saved.inputs.files.clone();
            let replacement = proposed.clone();
            let destination = workspace.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                for (path, text) in replacement {
                    let original = before.get(&path).context("unfrozen source replacement")?;
                    if original != &text {
                        crate::workspace_materialization::replace_bounded_regular_file(
                            &destination,
                            Path::new(&path),
                            original.as_bytes(),
                            text.as_bytes(),
                        )?;
                    }
                }
                Ok(())
            })
            .await??;
            let mut command = args(&["add", "--"]);
            command.extend(proposed.keys().map(std::ffi::OsString::from));
            run_git(&workspace, temporary.path(), command).await?;
            run_git(
                &workspace,
                temporary.path(),
                args(&["diff", "--cached", "--check"]),
            )
            .await?;
            let message = format!("Harness candidate {}", plan.id);
            run_git(
                &workspace,
                temporary.path(),
                args(&["commit", "--quiet", "-m", &message]),
            )
            .await?;
            journal.candidate = Some(source_snapshot(&workspace).await?);
            audit(&journal, &saved, &plan, temporary.path()).await?;
            journal.started = false;
            {
                let _lock = vault.acquire_lock()?;
                vault.write_mutable(&path, &journal)?;
            }
            Ok(journal)
        })
        .await;
    if result.is_err() {
        update_harness_cycle(&vault, id, |state| {
            if !matches!(
                state.stage,
                HarnessStage::Paused | HarnessStage::BudgetExhausted
            ) {
                state.stage = HarnessStage::RecoveryRequired;
            }
            state.reason =
                "Source checkout failed; private artifacts retained for inspection".into();
            Ok(())
        })?;
    }
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use codex_team::self_improvement::{start_harness_cycle, HarnessPlan};

    pub(crate) async fn fixture(temp: &Path) -> (LearningVault, HarnessPlan, PathBuf) {
        let repository = temp.join("repository");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        let (mut plan, mut inputs, proposal) = crate::harness_source_generation::tests::fixture();
        for (path, contents) in &inputs.files {
            std::fs::write(repository.join(path), contents).unwrap();
        }
        let home = temp.join("home");
        std::fs::create_dir(&home).unwrap();
        for values in [
            vec!["init", "-b", "main"],
            vec!["add", "."],
            vec!["commit", "-m", "fixture"],
        ] {
            run_git(&repository, temp, args(&values)).await.unwrap();
        }
        plan.baseline = source_snapshot(&repository).await.unwrap();
        plan.seal().unwrap();
        inputs.shared.plan_digest = plan.digest.clone();
        let journal = SourceGeneration {
            inputs_digest: canonical_digest(&inputs).unwrap(),
            proposal_digest: Some(canonical_digest(&proposal).unwrap()),
            result_digest: Some(
                canonical_digest(&proposal.apply(&inputs, &plan).unwrap()).unwrap(),
            ),
            inputs,
            proposal: Some(proposal),
        };
        let vault = LearningVault::open_at(temp.join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        vault
            .write_mutable(
                &format!("harness/source-generations/{}.json", plan.id),
                &journal,
            )
            .unwrap();
        (vault, plan, repository)
    }

    #[tokio::test]
    async fn hrs253_source_checkout_commits_only_private_proposed_bytes_and_reuses_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = fixture(temp.path()).await;
        let first = prepare(vault.clone(), &plan.id, &repository).await.unwrap();
        assert!(!first.started);
        let commit = first.candidate.as_ref().unwrap().head.clone();
        assert_ne!(commit, plan.baseline.head);
        assert_eq!(source_snapshot(&repository).await.unwrap(), plan.baseline);
        assert!(std::fs::read_to_string(repository.join("src/example.rs"))
            .unwrap()
            .contains("{ 0 }"));
        assert!(
            std::fs::read_to_string(first.workspace.join("src/example.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        let recovered = prepare(vault.clone(), &plan.id, &repository).await.unwrap();
        assert_eq!(recovered.candidate.as_ref().unwrap().head, commit);
        assert_eq!(recovered.workspace, first.workspace);
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.stage, HarnessStage::EvaluationFrozen);
        assert!(state.candidate_digest.is_none());
        assert!(!state.engineering_checks_passed);
        assert!(state.active_seconds > 0);
        assert!(state.active_reservations.is_empty());
        assert_eq!(state.settled_microusd, 0);
        assert_eq!(
            report(&vault, &plan.id).unwrap().unwrap()["candidate"]["head"],
            commit
        );
        std::fs::write(first.workspace.join("src/example.rs"), "tampered").unwrap();
        assert!(prepare(vault.clone(), &plan.id, &repository).await.is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::RecoveryRequired
        );
        assert_eq!(source_snapshot(&repository).await.unwrap(), plan.baseline);
    }

    #[tokio::test]
    async fn hrs253_source_checkout_interruption_and_unknown_charges_never_replay() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = fixture(temp.path()).await;
        let mut journal = prepare(vault.clone(), &plan.id, &repository).await.unwrap();
        let commit = journal.candidate.as_ref().unwrap().head.clone();
        journal.started = true;
        vault
            .write_mutable(
                &format!("harness/source-checkouts/{}.json", plan.id),
                &journal,
            )
            .unwrap();
        assert!(prepare(vault.clone(), &plan.id, &repository).await.is_err());
        assert_eq!(
            source_snapshot(&journal.workspace).await.unwrap().head,
            commit
        );
        assert_eq!(source_snapshot(&repository).await.unwrap(), plan.baseline);
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::RecoveryRequired
        );

        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = fixture(temp.path()).await;
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reserve(&plan, "unknown", 123)
        })
        .unwrap();
        assert!(prepare(vault.clone(), &plan.id, &repository).await.is_err());
        assert!(report(&vault, &plan.id).unwrap().is_none());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.reservations["unknown"],
            123
        );
    }

    #[test]
    fn hrs253_source_replacement_never_truncates_wrong_preimages_or_link_targets() {
        use crate::workspace_materialization::replace_bounded_regular_file as replace;
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        let file = temp.path().join("src/file.rs");
        std::fs::write(&file, b"before").unwrap();
        assert!(replace(temp.path(), Path::new("src/file.rs"), b"wrong!", b"after").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"before");
        std::os::unix::fs::symlink("file.rs", temp.path().join("src/link.rs")).unwrap();
        assert!(replace(temp.path(), Path::new("src/link.rs"), b"before", b"after").is_err());
        std::os::unix::fs::symlink("src", temp.path().join("alias")).unwrap();
        assert!(replace(temp.path(), Path::new("alias/file.rs"), b"before", b"after").is_err());
        std::fs::hard_link(&file, temp.path().join("src/hard.rs")).unwrap();
        assert!(replace(temp.path(), Path::new("src/file.rs"), b"before", b"after").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"before");
        std::fs::remove_file(temp.path().join("src/hard.rs")).unwrap();
        replace(temp.path(), Path::new("src/file.rs"), b"before", b"after").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"after");
    }
}
